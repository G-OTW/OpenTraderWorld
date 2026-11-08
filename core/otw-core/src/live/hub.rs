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
use super::outage::{self, Outage};
use super::{
    fault_message, fault_of, missing_buckets, stream_connector_for, FeedUpdate, Instrument,
    LiveEvent, LiveFault, LiveQuote, Payload, SessionRequest, SourceGrain, SourcePref, SEED_BARS,
};

/// What a viewer's SSE stream carries. Bars are the point; the status is what makes a feed
/// that cannot run *legible* , before this the only symptom of a rejected key was a dot that
/// stayed grey while the supervisor reconnected forever.
#[derive(Debug, Clone)]
pub enum LiveMsg {
    Bar(LiveEvent),
    Status(LiveStatus),
    /// Only on a quote subscription ([`Instrument::quote`]), whose channel is its own: a
    /// burst of quotes can make a slow viewer skip quotes, never bars.
    Quote(LiveQuote),
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
/// A provider can publish a bar minutes after its period (Coinbase's minutes). A settle
/// re-read that comes back without one of its periods asks again this long after, this many
/// times in all, rather than leave the fold's approximation stored.
const SETTLE_RETRY: Duration = Duration::from_secs(60);
const SETTLE_TRIES: u32 = 3;

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
    /// Candles (every chart pane) or the venue's trade prints (live alerts). See
    /// [`SourcePref`].
    pub source: SourcePref,
}

impl LiveTarget {
    /// The instrument, as the transport layer addresses it.
    pub fn instrument(&self) -> Instrument {
        Instrument::new(&self.ticker, &self.asset_type, &self.timeframe).with_source(self.source)
    }

    /// Identity of the *subscription*: an instrument at a timeframe. The socket it rides on
    /// is a separate key (see [`session_key`]).
    pub fn key(&self) -> String {
        self.instrument().key()
    }
}

/// Whether a tick preference changes what this instrument is fed from.
fn honors_ticks(connector: &dyn super::StreamConnector, ticks: &Instrument) -> bool {
    let candles = ticks.clone().with_source(SourcePref::Candles);
    match (connector.grain(ticks), connector.grain(&candles)) {
        (Ok(t), Ok(c)) => t != c,
        _ => false,
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
    /// The latest quote, on a quote subscription.
    quote: Arc<Mutex<Option<LiveQuote>>>,
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
            quote: self.quote.clone(),
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
    quote: Arc<Mutex<Option<LiveQuote>>>,
    status: Arc<Mutex<LiveStatus>>,
}

enum HubCmd {
    Add(Box<SubSpec>),
    Remove(String),
    /// Bars or a tick supplied from outside (a push source's webhook), for the
    /// subscriptions on these coordinates.
    Ingest(Box<IngestCmd>),
}

/// What an external source hands the hub: bars at one timeframe, or one trade print.
#[derive(Debug, Clone)]
pub enum Ingest {
    Bars { timeframe: String, bars: Vec<Bar> },
    Tick { price: f64, size: f64, ts: OffsetDateTime },
}

struct IngestCmd {
    symbol: String,
    asset_type: String,
    what: Ingest,
}

/// The newest bars of a pushed batch that are broadcast live. The rest is history: written
/// by the webhook, read by the chart from the series endpoint, and too many to send down a
/// viewer's channel at once.
const INGEST_LIVE_BARS: usize = 50;

/// Whether a subscription reads what was pushed for these coordinates.
fn wants(target: &LiveTarget, symbol: &str, asset_type: &str, what: &Ingest) -> bool {
    target.ticker == symbol
        && target.asset_type == asset_type
        && target.timeframe != super::QUOTE_TF
        && match what {
            Ingest::Bars { timeframe, .. } => &target.timeframe == timeframe || folds_from(&target.timeframe, timeframe).is_some(),
            Ingest::Tick { .. } => true,
        }
}

/// The seconds a pushed bar at `src` covers, when a subscription at `tf` is a whole number
/// of them (a 1m bar into 1h): the subscription folds it in as a partial bar.
fn folds_from(tf: &str, src: &str) -> Option<i64> {
    let (Ok(tf), Ok(src)) = (crate::histdata::timeframe_secs(tf), crate::histdata::timeframe_secs(src)) else { return None };
    (src > 0 && tf > src && tf % src == 0).then_some(src)
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
    /// New observations of value series (key `fund:<series_id>`), for the charts drawing them.
    points: broadcast::Sender<PointEvent>,
}

/// A new or revised observation of a Fundamentals series: what a chart's data pane appends as
/// one more step. Not a candle: no fold, no forming state, no Bucketer.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PointEvent {
    /// `fund:<series_id>`: the key a viewer subscribes on.
    pub key: String,
    pub series_id: Uuid,
    /// The period's start, epoch ms (midnight UTC of its date).
    pub period_start: i64,
    /// When it was released, epoch ms, if the source says.
    pub released_at: Option<i64>,
    /// When it can be read, epoch ms: its release, else the end of its period.
    pub known_at: i64,
    pub value: f64,
}

/// The hub key of a value series.
pub fn point_key(series_id: Uuid) -> String {
    format!("fund:{series_id}")
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
    /// The latest quote, on a quote subscription: a quiet book can be still for seconds, and
    /// the viewer should not wait for it to move.
    pub quote: Option<LiveQuote>,
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
            points: broadcast::channel(256).0,
        }))
    }

    /// Tell the charts drawing a value series that a period was added or revised. Nobody
    /// listening is not an error: the observation is stored either way.
    pub fn publish_point(&self, series_id: Uuid, period_start: time::Date, released_at: Option<OffsetDateTime>, known_at: i64, value: f64) {
        let ms = |t: OffsetDateTime| (t.unix_timestamp_nanos() / 1_000_000) as i64;
        let _ = self.0.points.send(PointEvent {
            key: point_key(series_id),
            series_id,
            period_start: ms(OffsetDateTime::new_utc(period_start, time::Time::MIDNIGHT)),
            released_at: released_at.map(ms),
            known_at,
            value,
        });
    }

    /// Every point event; the reader keeps the keys it watches.
    pub fn subscribe_points(&self) -> broadcast::Receiver<PointEvent> {
        self.0.points.subscribe()
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

    /// Hand bars or a tick from an external source (a push source's webhook) to every live
    /// subscription of this connector on these coordinates, through the same fold, status
    /// and broadcast a socket's updates go through. Returns how many subscriptions it reached;
    /// zero means nobody is watching, and a tick then goes nowhere.
    pub fn ingest(&self, connector_id: Uuid, symbol: &str, asset_type: &str, what: Ingest) -> usize {
        let prefix = format!("{connector_id}|");
        let sessions = self.0.sessions.lock().unwrap();
        let mut reached = 0;
        for (key, entry) in sessions.iter().filter(|(k, e)| k.starts_with(&prefix) && !e.task.is_finished()) {
            let n = entry.subs.values().filter(|s| wants(&s.target, symbol, asset_type, &what)).count();
            if n == 0 {
                continue;
            }
            let cmd = IngestCmd { symbol: symbol.to_string(), asset_type: asset_type.to_string(), what: what.clone() };
            if entry.cmds.send(HubCmd::Ingest(Box::new(cmd))).is_ok() {
                reached += n;
            } else {
                tracing::debug!("live ingest: session {key} is gone");
            }
        }
        reached
    }

    /// A viewer handle on a channel the test feeds itself.
    #[cfg(test)]
    pub(crate) fn detached_subscription(&self, rx: broadcast::Receiver<LiveMsg>) -> Subscription {
        Subscription {
            rx,
            snapshot: None,
            quote: None,
            status: LiveStatus::live(),
            guard: LiveGuard { hub: self.clone(), session: String::new(), sub: String::new(), gen: 0 },
        }
    }

    fn next_gen(&self) -> u64 {
        let mut g = self.0.next_gen.lock().unwrap();
        *g += 1;
        *g
    }

    /// Start (or join) the live feed for an instrument and return a viewer subscription.
    /// Errors only if the provider has no live feed, or cannot say which socket the
    /// instrument belongs on.
    pub fn subscribe(&self, mut target: LiveTarget) -> Result<Subscription> {
        let connector = stream_connector_for(&target.provider)
            .ok_or_else(|| anyhow!("provider {} has no live feed", target.provider))?;
        // Asking for ticks where the venue would serve the same grain anyway (no print
        // stream, or prints already: Coinbase, Interactive Brokers) is asking for candles, so
        // the alert joins the pane's subscription instead of running a second fold.
        if target.source == SourcePref::Ticks && !honors_ticks(connector.as_ref(), &target.instrument()) {
            target.source = SourcePref::Candles;
        }
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
                    // A viewer that records nothing (a paper watch, an alert, an unsaved chart)
                    // joins whatever runs: restarting a recording subscription for it would cut
                    // its other viewers off, and they would cut it off in turn when they came
                    // back, each knocking the other off the feed.
                    if sub.dataset_id == target.dataset_id || target.dataset_id.is_none() {
                        sub.viewers += 1;
                        return Ok(Subscription {
                            rx: sub.tx.subscribe(),
                            snapshot: sub.current.lock().unwrap().clone(),
                            quote: *sub.quote.lock().unwrap(),
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
            quote: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(LiveStatus::connecting())),
            viewers: 1,
            dataset_id,
            target,
            gen,
        };
        let subscription = Subscription {
            rx,
            snapshot: None,
            quote: None,
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
    quote: Arc<Mutex<Option<LiveQuote>>>,
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
    /// The span still owed by a gap backfill: one found while another was in flight, or one
    /// whose request failed. Sent at the next closed bar rather than dropped, which is what
    /// used to happen to both, leaving a hole stored and drawn as if the bars were adjacent.
    gap_pending: Arc<Mutex<Option<(OffsetDateTime, OffsetDateTime)>>>,
    /// The settle re-read's own flag. It used to share the one above, which meant a gap
    /// backfill in flight silently swallowed the re-read and left the folded session bar as
    /// the stored row: the two are different jobs and neither may cancel the other.
    settling: Arc<std::sync::atomic::AtomicBool>,
    /// Where a re-read hands the provider's own bar back, so the fold re-anchors on it (a
    /// venue's DST switch) and the hub refreshes its remembered [`Anchor`].
    anchor_tx: mpsc::UnboundedSender<(String, Bar)>,
    /// The oldest closed bar still waiting for its settle re-read. A re-read in flight holds
    /// `settling`, so the bars closing meanwhile queue here and go out in the next one.
    settle_from: Option<OffsetDateTime>,
    /// Whether the provider has a REST side to seed, backfill and settle from (a push
    /// source has none: asking it would only log the same refusal on every bar).
    rest: bool,
    /// Where a session-shaped bucket starts when there is no REST bar to anchor on.
    anchor_offset: Option<i64>,
    /// The newest pushed bar of a finer timeframe already folded into this subscription, so
    /// a batch posted again does not add its bars twice.
    pushed_upto: Option<OffsetDateTime>,
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
    let mut outage = Outage::default();
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
            &mut outage,
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
                        *run.quote.lock().unwrap() = None;
                    }
                }
                if fault.terminal() {
                    return;
                }
                // A seat someone else is holding clears on its own, but not in a second.
                if fault == LiveFault::Conflict {
                    backoff = BACKOFF_MAX;
                }
                outage.down(&message);
                if let Some((reason, down)) = outage.due() {
                    let text = outage::down_text(&meta.connector, &meta.provider, &tickers(&subs), &reason, down);
                    outage::notify(inner.pool.clone(), inner.cipher.clone(), inner.http.clone(), text);
                }
            }
        }
        for run in subs.values_mut() {
            *run.current.lock().unwrap() = None;
            *run.quote.lock().unwrap() = None;
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

/// The instruments a session carries, as a notification names them.
fn tickers(subs: &HashMap<String, SubRun>) -> String {
    let mut t: Vec<&str> = subs.values().map(|s| s.target.ticker.as_str()).collect();
    t.sort_unstable();
    t.dedup();
    t.join(", ")
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
        // A quote has no period: nothing to fold, seed or record.
        Err(_) if inst.is_quote() => 0,
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
    let anchor_offset = (tf > 0).then(|| connector.anchor_offset(&inst, tf, OffsetDateTime::now_utc())).flatten();
    Some(SubRun {
        rest: connector.has_rest(),
        anchor_offset,
        agg,
        src_secs,
        target: spec.target,
        inst,
        channel,
        tf,
        tx: spec.tx,
        current: spec.current,
        quote: spec.quote,
        status: spec.status,
        last_closed: None,
        seeding: false,
        buffer: Vec::new(),
        stopped: false,
        backfilling: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        gap_pending: Arc::new(Mutex::new(None)),
        settling: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        anchor_tx,
        settle_from: None,
        pushed_upto: None,
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
        // Between two sessions there is nothing live to hand it to.
        HubCmd::Ingest(_) => {}
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
    outage: &mut Outage,
) -> Result<()> {
    // A push source holds no credential and reads nothing from the store to open.
    let secrets = if connector.has_rest() {
        load_secrets(inner, meta).await?
    } else {
        std::collections::HashMap::new()
    };
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
            pool: Some(&inner.pool),
            connector_id: Some(meta.connector_id),
        })
        .await?;
    let mut stream = session.stream;
    let handle = session.handle;
    for run in subs.values() {
        if !run.stopped {
            run.publish(LiveStatus::live());
        }
    }
    outage.up();

    // Seeds run off the event loop: a socket that is not being read is a socket the provider
    // drops, and a workspace opening four recording panes at once would otherwise pause
    // everything for four REST round trips.
    let mut seeds: JoinSet<(String, Result<Vec<Bar>>, Option<OffsetDateTime>)> = JoinSet::new();
    for (key, run) in subs.iter_mut() {
        begin_sync(inner, &secrets, key.clone(), run, &mut seeds);
    }

    let mut timer = tokio::time::interval(TICK);
    // A socket that holds this long ends an outage; one that drops before does not, so a
    // handshake that passes every reconnect is not a "back" every reconnect.
    let held = tokio::time::sleep(outage::STEADY);
    tokio::pin!(held);
    let mut held_done = false;
    loop {
        tokio::select! {
            _ = &mut held, if !held_done => {
                held_done = true;
                if let Some(down) = outage.steady() {
                    let text = outage::back_text(&meta.connector, &meta.provider, &tickers(subs), down);
                    outage::notify(inner.pool.clone(), inner.cipher.clone(), inner.http.clone(), text);
                }
            }
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
                Some(HubCmd::Ingest(cmd)) => ingest(inner, &secrets, meta, subs, *cmd).await,
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
///   - **nothing to record, epoch-bucketed, and published by the venue itself**: no request
///     at all, the series is known whole up to the bucket before the current one;
///   - **nothing to record, session-shaped, and the hub still remembers the alignment**: no
///     request either, the remembered [`Anchor`] is what the seed was for;
///   - **anything else**: the REST seed (a recording subscription must write the gap, a cold
///     session-shaped fold has to be told where the day starts, and any fold has to adopt
///     the period in progress rather than start it from the first tick after connect).
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
    if run.stopped || run.inst.is_quote() {
        return;
    }
    // Nothing to seed from: a fold starts at the source's own period boundary, and the
    // series is whatever the source posts.
    if !run.rest {
        if let (Some(offset), Some(agg)) = (run.anchor_offset, run.agg.as_mut()) {
            agg.anchor_at(offset);
        }
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
    } else if !records && run.agg.is_none() {
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
        // Read before the provider is asked, and handed back whatever it answers: a seed that
        // failed still knows where the recorded series stops. Losing it made the hub fall back
        // to "whole up to the previous bucket", so the hole the outage left was never
        // backfilled and the chart drew the bars either side of it as neighbours.
        let stored = match dataset_id {
            Some(id) => match store::latest_bar_ts(&inner.pool, id).await {
                Ok(s) => s,
                Err(e) => return (key, Err(e.into()), None),
            },
            None => None,
        };
        let out = async {
            let rest = crate::histdata::connector_for(&target.provider)?;
            let now = OffsetDateTime::now_utc();
            let from =
                stored.unwrap_or_else(|| seed_from(now, tf, seed_bar_count(rest.as_ref())));
            // The period in progress included: a fold adopts it (see `finish_seed`).
            let seed = rest
                .fetch_seed(
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
                // The running period is streamed and recorded as it closes: store the closed ones.
                let closed = crate::histdata::closed_only(crate::histdata::Chunk::of(seed.clone()), &target.timeframe, now);
                if !closed.bars.is_empty() {
                    store::write_bars(&inner.pool, id, &closed.bars).await?;
                }
            }
            Ok::<_, anyhow::Error>(seed)
        }
        .await;
        (key, out, stored)
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
    let mut primed: Option<OffsetDateTime> = None;
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
            Ok(mut bars) => {
                // The seed carries the period in progress. It is the provider's own partial
                // bar: the honest open and range so far, sent as forming and never as closed.
                let tf = run.tf;
                let now = OffsetDateTime::now_utc();
                let running = bars.last().filter(|b| b.ts + TimeDuration::seconds(tf) > now).cloned();
                if running.is_some() {
                    bars.pop();
                }
                // Seeded bars are broadcast, not just written. The viewer loaded its history
                // from the series endpoint *before* this socket opened, so a bar that closed
                // in between is in the store and nowhere on the chart. Re-sending it is
                // idempotent: the client keys on the timestamp and replaces in place.
                for b in &bars {
                    let _ = run.tx.send(LiveMsg::Bar(bar_to_event(b)));
                }
                let seed_last = bars.last().map(|b| b.ts);
                // A session-shaped fold takes its alignment from the bars themselves.
                if anchored(tf) {
                    if let Some(last) = running.as_ref().or(bars.last()) {
                        // Learned once, reused by every pane that opens on these coordinates
                        // for the next few hours instead of paying for the same request.
                        inner.remember_anchor(&run.target, tf, last);
                        if let Some(agg) = run.agg.as_mut() {
                            agg.anchor(last.ts);
                        }
                    }
                }
                if let Some(r) = &running {
                    let forming = LiveEvent { closed: false, ..bar_to_event(r) };
                    // A fold adopts the period already in progress rather than starting a new
                    // bucket from the next tick: its open and range belong to the period, not
                    // to the moment the socket opened. What it folded while the seed was in
                    // flight is in the provider's bar already.
                    if let Some(agg) = run.agg.as_mut() {
                        agg.prime(forming.clone());
                        primed = Some(r.ts);
                    }
                    *run.current.lock().unwrap() = Some(forming.clone());
                    let _ = run.tx.send(LiveMsg::Bar(forming));
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
    // The overlap merge: anything the seed already covers is history, not an update. The
    // primed period's buffered updates are the fold since connect, which the prime replaced.
    for e in buffered {
        if let Some(sl) = seed_last {
            if e.bar_ts < sl {
                continue;
            }
        }
        if primed == Some(e.bar_ts) && !e.closed {
            continue;
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

    // Anything else is the channel speaking again: a line that came back from a passing fault
    // (IB places an ended subscription again on its own) is live, not still retrying.
    for key in &keys {
        if let Some(run) = subs.get(key) {
            run.publish(LiveStatus::live());
        }
    }

    if let Payload::Quote(q) = &update.payload {
        for key in &keys {
            if let Some(run) = subs.get(key) {
                publish_quote(run, *q);
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

/// Hand externally supplied bars or a tick to the subscriptions on their coordinates.
///
/// A tick goes through the subscription's fold exactly like a trade off a socket, so every
/// timeframe on screen gets its forming candle and a closed bar is recorded when the
/// subscription carries a dataset. Bars at the subscription's own timeframe are the source's
/// own candles: broadcast as they are, the newest still forming when its period is not over,
/// and set as the fold's state for their period. A coarser subscription folds them in as
/// partial bars. They are not written here: the webhook has already written the closed ones.
async fn ingest(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    subs: &mut HashMap<String, SubRun>,
    cmd: IngestCmd,
) {
    let keys: Vec<String> = subs
        .iter()
        .filter(|(_, s)| !s.stopped && wants(&s.target, &cmd.symbol, &cmd.asset_type, &cmd.what))
        .map(|(k, _)| k.clone())
        .collect();
    for key in keys {
        if let Some(run) = subs.get(&key) {
            run.publish(LiveStatus::live());
        }
        match &cmd.what {
            Ingest::Tick { price, size, ts } => {
                let events = match subs.get_mut(&key).and_then(|r| r.agg.as_mut()) {
                    Some(agg) => agg.trade(*price, *size, *ts),
                    None => Vec::new(),
                };
                for e in events {
                    deliver(inner, secrets, meta, subs, &key, e).await;
                }
            }
            Ingest::Bars { timeframe, bars } => {
                let Some(run) = subs.get_mut(&key) else { continue };
                let Ok(tf) = crate::timeframe::Timeframe::parse(timeframe) else { continue };
                let now = OffsetDateTime::now_utc();
                let tail = &bars[bars.len().saturating_sub(INGEST_LIVE_BARS)..];
                if let Some(src_secs) = folds_from(&run.target.timeframe, timeframe) {
                    // A coarser pane folds the bars of its period in progress as partial bars,
                    // each once; older ones are history its chart already read.
                    let mut events = Vec::new();
                    if let Some(agg) = run.agg.as_mut() {
                        let bucket = agg.bucket_of(now);
                        let upto = run.pushed_upto;
                        for b in tail.iter().filter(|b| b.ts >= bucket && upto.is_none_or(|u| b.ts > u)) {
                            let e = LiveEvent { closed: tf.closes_at(b.ts) <= now, provider_ts: now, ..bar_to_event(b) };
                            events.extend(agg.bar(e, src_secs));
                            run.pushed_upto = Some(b.ts);
                        }
                    }
                    for e in events {
                        deliver(inner, secrets, meta, subs, &key, e).await;
                    }
                    continue;
                }
                for b in tail {
                    let e = LiveEvent { closed: tf.closes_at(b.ts) <= now, provider_ts: now, ..bar_to_event(b) };
                    // The source's candle is the truth for its period: a forming one primes the
                    // fold, so a tick that follows extends it (its open and range kept), and a
                    // closed one ends whatever the fold held for it.
                    if let Some(agg) = run.agg.as_mut() {
                        if e.closed {
                            agg.settle(e.bar_ts);
                        } else {
                            agg.prime(e.clone());
                        }
                    }
                    {
                        let mut cur = run.current.lock().unwrap();
                        if !e.closed {
                            *cur = Some(e.clone());
                        } else if cur.as_ref().is_some_and(|c| c.bar_ts <= e.bar_ts) {
                            *cur = None;
                        }
                    }
                    if e.closed && run.last_closed.is_none_or(|lc| e.bar_ts > lc) {
                        run.last_closed = Some(e.bar_ts);
                    }
                    let _ = run.tx.send(LiveMsg::Bar(e));
                }
            }
        }
    }
}

/// Hand a quote to its viewers when the price moved. A book ticker also fires on size
/// changes alone, which the chart does not draw: most of a busy pair's frames stop here.
fn publish_quote(run: &SubRun, q: LiveQuote) {
    if !run.inst.is_quote() {
        return;
    }
    let mut last = run.quote.lock().unwrap();
    if last.is_some_and(|l| l.bid == q.bid && l.ask == q.ask) {
        return;
    }
    *last = Some(q);
    let _ = run.tx.send(LiveMsg::Quote(q));
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
    // A closed bar older than the newest one already closed is a straggler (a venue answering
    // late): recorded and resent in place, but it never pulls the series back, so the next
    // close does not owe a gap for a bar that was received.
    let straggler = e.closed && run.last_closed.is_some_and(|lc| e.bar_ts < lc);
    if straggler {
        if let Some(dataset_id) = run.target.dataset_id {
            store::write_bars(&inner.pool, dataset_id, &[e.as_bar()]).await?;
        }
    } else if e.closed {
        // Technical gap: closed bar skipped one or more buckets → backfill just that span.
        //
        // **Epoch-bucketed timeframes only.** On a session-shaped one every non-session day
        // counts as a missing bucket, so a weekend or a holiday looks exactly like a dropped
        // bar: Monday's first close would fire a REST request for a Saturday that was never
        // going to exist. What actually closes a hole there is the seed on the next
        // reconnect, which asks from stored history rather than from a calendar we do not
        // have.
        if !anchored(run.tf) && run.rest {
            if let Some(prev) = run.last_closed {
                if !missing_buckets(prev, e.bar_ts, run.tf).is_empty() {
                    owe_gap(&run.gap_pending, prev + TimeDuration::seconds(run.tf), e.bar_ts);
                }
            }
            send_owed_gap(inner, secrets, meta, run);
        }
        if let Some(dataset_id) = run.target.dataset_id {
            store::write_bars(&inner.pool, dataset_id, &[e.as_bar()]).await?;
        }
        // The fold is an approximation for a period the provider delimits itself: it cannot
        // know the closing print, and it counts extended-hours minutes the official session
        // bar leaves out. Re-read that one bar once it has settled and let the provider's own
        // values overwrite it, on screen as much as in the store — a pane that records
        // nothing is looking at the same approximation.
        //
        // An intraday fold is re-read when it is recorded: a trade after the grace, a
        // reconnect inside a period or a tick feed that is not the venue's tape all leave the
        // stored row short. Bars closing while a re-read is in flight queue for the next one.
        if run.rest && (anchored(run.tf) || run.agg.is_some() && run.target.dataset_id.is_some()) {
            let from = *run.settle_from.get_or_insert(e.bar_ts);
            if spawn_settle_reread(inner, secrets, meta, run, from, e.bar_ts + TimeDuration::seconds(run.tf)) {
                run.settle_from = None;
            }
        }
        run.last_closed = Some(e.bar_ts);
        let mut cur = run.current.lock().unwrap();
        if cur.as_ref().is_some_and(|c| c.bar_ts <= e.bar_ts) {
            *cur = None;
        }
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
/// is logged, not fatal, and only one runs at a time per subscription. A span it could not
/// send or could not fetch stays owed ([`SubRun::gap_pending`]).
fn spawn_gap_backfill(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    run: &SubRun,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> bool {
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
            // A gap can be a period nobody traded: one answer is the answer.
            tries: 1,
            owed: Some(run.gap_pending.clone()),
        },
    )
}

/// Add `[from, to)` to the span a gap backfill still owes, widening what is already there.
fn owe_gap(
    owed: &Mutex<Option<(OffsetDateTime, OffsetDateTime)>>,
    from: OffsetDateTime,
    to: OffsetDateTime,
) {
    let mut o = owed.lock().unwrap();
    *o = Some(match *o {
        Some((f, t)) => (f.min(from), t.max(to)),
        None => (from, to),
    });
}

/// Send the owed span, unless a backfill is already running: then it waits for the next
/// closed bar. Taken before the spawn, so a request failing at once puts it back rather than
/// racing the take.
fn send_owed_gap(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    run: &SubRun,
) {
    let Some((from, to)) = run.gap_pending.lock().unwrap().take() else { return };
    if !spawn_gap_backfill(inner, secrets, meta, run, from, to) {
        owe_gap(&run.gap_pending, from, to);
    }
}

/// Re-read folded bars once the provider has finalized them, and re-anchor a session-shaped
/// fold on what comes back. False when a re-read is already in flight: nothing was sent.
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
) -> bool {
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
            reanchor: anchored(run.tf),
            tries: SETTLE_TRIES,
            owed: None,
        },
    )
}

/// One detached REST re-read: which span, how long to wait first, which in-flight flag it
/// holds, and whether its answer re-anchors the fold.
struct Refetch {
    from: OffsetDateTime,
    to: OffsetDateTime,
    delay: Duration,
    flag: Arc<std::sync::atomic::AtomicBool>,
    reanchor: bool,
    /// Asks in all while a period of the span is missing from the answer.
    tries: u32,
    /// Where a failed request puts its span back, to be asked again (gap backfills only).
    owed: Option<Arc<Mutex<Option<(OffsetDateTime, OffsetDateTime)>>>>,
}

fn spawn_refetch(
    inner: &Arc<Inner>,
    secrets: &std::collections::HashMap<String, String>,
    meta: &LiveTarget,
    run: &SubRun,
    job: Refetch,
) -> bool {
    use std::sync::atomic::Ordering;
    let Refetch { from, to, delay, flag, reanchor, tries, owed } = job;
    if flag.swap(true, Ordering::SeqCst) {
        return false;
    }
    let inner = inner.clone();
    let secrets = secrets.clone();
    let target = run.target.clone();
    let connector_name = meta.connector.clone();
    let tx = run.tx.clone();
    let anchor = reanchor.then(|| (run.anchor_tx.clone(), run.inst.key()));
    // The periods the span must come back with. A session-shaped one is stamped by the
    // provider, so only "something came back" can be checked there.
    let step = (!anchored(run.tf)).then_some(run.tf);
    tokio::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        for attempt in 1..=tries.max(1) {
            if attempt > 1 {
                tokio::time::sleep(SETTLE_RETRY).await;
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
                let missing = match step {
                    Some(tf) => {
                        let got: std::collections::HashSet<OffsetDateTime> = bars.iter().map(|b| b.ts).collect();
                        let mut t = from;
                        let mut missing = false;
                        while t < to {
                            missing |= !got.contains(&t);
                            t += TimeDuration::seconds(tf);
                        }
                        missing
                    }
                    None => bars.is_empty(),
                };
                Ok::<_, anyhow::Error>(missing)
            }
            .await;
            match outcome {
                Ok(true) if attempt < tries => continue,
                Ok(true) if tries > 1 => tracing::warn!(
                    "live re-read {} ({connector_name}): the provider has not published {from}..{to} after {tries} tries",
                    target.ticker
                ),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("live backfill {} ({connector_name}): {e:#}", target.ticker);
                    if let Some(owed) = &owed {
                        owe_gap(owed, from, to);
                    }
                }
            }
            break;
        }
        flag.store(false, Ordering::SeqCst);
    });
    true
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
#[path = "hub_stream_tests.rs"]
mod stream_tests;

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
            source: SourcePref::Candles,
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

    /// A chart pane and a live alert on the same symbol: one socket, two subscriptions (the
    /// pane on the venue's klines, the alert folding the prints). Where the venue would feed
    /// both the same way, the alert simply joins the pane's subscription.
    #[tokio::test]
    async fn a_pane_and_an_alert_on_one_symbol_share_one_socket() {
        let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
        let hub = LiveHub::new(pool, Client::new(), SecretCipher::from_master("t").unwrap());
        let key = Uuid::new_v4();
        let pane = target("binance", key, "BTCUSDT", "1m");
        let alert = LiveTarget { source: SourcePref::Ticks, ..pane.clone() };
        let _p = hub.subscribe(pane.clone()).unwrap();
        let _a = hub.subscribe(alert.clone()).unwrap();
        let _a2 = hub.subscribe(alert.clone()).unwrap();
        {
            let sessions = hub.0.sessions.lock().unwrap();
            assert_eq!(sessions.len(), 1, "one socket");
            let subs = &sessions.values().next().unwrap().subs;
            assert_eq!(subs.len(), 2, "the pane's klines and the alerts' prints");
            assert_eq!(subs[&pane.key()].viewers, 1);
            assert_eq!(subs[&alert.key()].viewers, 2, "two alerts on one symbol share the prints");
            assert_eq!(subs[&alert.key()].target.source, SourcePref::Ticks);
        }
        let binance = stream_connector_for("binance").unwrap();
        assert_eq!(binance.channel_of(&pane.instrument()).unwrap(), "BTCUSDT|1m");
        assert_eq!(binance.channel_of(&alert.instrument()).unwrap(), "BTCUSDT|trades");
        assert_eq!(hub.lines_held(key), 1, "one instrument, whatever it is fed from");

        // Coinbase streams prints for the pane already: the alert joins it.
        let cb = target("coinbase", Uuid::new_v4(), "BTC-USD", "1m");
        let _c = hub.subscribe(cb.clone()).unwrap();
        let _c2 = hub.subscribe(LiveTarget { source: SourcePref::Ticks, ..cb.clone() }).unwrap();
        let sessions = hub.0.sessions.lock().unwrap();
        let subs = &sessions.values().find(|e| e.subs.contains_key(&cb.key())).unwrap().subs;
        assert_eq!((subs.len(), subs[&cb.key()].viewers, subs[&cb.key()].target.source), (1, 2, SourcePref::Candles));
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
            quote: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(LiveStatus::connecting())),
        };
        build_run(connector.as_ref(), spec, anchor_tx).unwrap()
    }

    /// A quote is a subscription of its own: its own channel (so a burst of quotes or a
    /// refusal of them never reaches the bars), nothing to fold, and one per instrument
    /// whatever the timeframes on screen.
    #[test]
    fn a_quote_is_its_own_channel_on_the_same_socket() {
        let q = run_of("binance", "crypto", "BTCUSDT", super::super::QUOTE_TF);
        let bars = run_of("binance", "crypto", "BTCUSDT", "1m");
        assert!(q.inst.is_quote());
        assert_eq!((q.tf, q.agg.is_none()), (0, true));
        assert_ne!(q.channel, bars.channel);
        assert_eq!(q.target.key(), Instrument::quote("BTCUSDT", "crypto").key());
        let session_of = |p: &str, t: &LiveTarget| {
            let c = stream_connector_for(p).unwrap();
            session_key(t.connector_id, &c.socket_class(&t.instrument()).unwrap())
        };
        let key = Uuid::new_v4();
        let quote = target("binance", key, "BTCUSDT", super::super::QUOTE_TF);
        assert_eq!(session_of("binance", &quote), session_of("binance", &target("binance", key, "BTCUSDT", "1m")), "spot: one socket");
        // USDⓈ-M serves its book ticker on another route: a socket of its own.
        let fq = target("binance_futures", key, "BTCUSDT", super::super::QUOTE_TF);
        assert_ne!(session_of("binance_futures", &fq), session_of("binance_futures", &target("binance_futures", key, "BTCUSDT", "1m")));
        // An instrument's lines are counted once, quote or not.
        assert_eq!(lines_of([quote.clone(), target("binance", key, "BTCUSDT", "1m")].iter(), key), 1);
    }

    #[test]
    fn a_quote_goes_out_only_when_the_price_moves() {
        let run = run_of("binance", "crypto", "BTCUSDT", super::super::QUOTE_TF);
        let mut rx = run.tx.subscribe();
        let at = OffsetDateTime::now_utc();
        let q = LiveQuote::new(100.0, 100.5, at).unwrap();
        publish_quote(&run, q);
        // A size change alone (same prices) is not news for a chart.
        publish_quote(&run, LiveQuote::new(100.0, 100.5, at + TimeDuration::milliseconds(5)).unwrap());
        publish_quote(&run, LiveQuote::new(100.0, 100.6, at).unwrap());
        let got: Vec<f64> = std::iter::from_fn(|| match rx.try_recv() {
            Ok(LiveMsg::Quote(q)) => Some(q.ask),
            _ => None,
        })
        .collect();
        assert_eq!(got, vec![100.5, 100.6]);
        assert_eq!(run.quote.lock().unwrap().map(|q| q.ask), Some(100.6), "the latest is kept for the next viewer");
        // A bar subscription never carries one.
        let bars = run_of("binance", "crypto", "BTCUSDT", "1m");
        let mut brx = bars.tx.subscribe();
        publish_quote(&bars, q);
        assert!(brx.try_recv().is_err());
    }

    /// No quote stream, no lines: nothing is derived from the candles.
    #[test]
    fn a_venue_without_a_quote_stream_refuses_it() {
        assert_eq!(super::super::quote_source("binance", "crypto", "BTCUSDT"), Some("native"));
        assert_eq!(super::super::quote_source("binance_futures", "crypto", "BTCUSDT"), Some("native"));
        assert_eq!(super::super::quote_source("capitalcom", "fx", "GOLD"), Some("candle_sides"));
        for p in ["bybit", "okx", "bitget", "gateio", "kucoin", "mexc", "bitfinex", "deribit", "kraken", "coinbase", "alpaca", "massive"] {
            assert!(super::super::quote_source(p, "crypto", "X").is_some(), "{p}");
        }
        // Interactive Brokers: a line of its own per instrument, apart from the prints; an
        // index has no bid or ask.
        let ibkr = stream_connector_for("ibkr").unwrap();
        assert_eq!(super::super::quote_source("ibkr", "equity", "AAPL"), Some("native"));
        assert_eq!(super::super::quote_source("ibkr", "index", "SPX"), None);
        assert_ne!(ibkr.channel_of(&Instrument::quote("AAPL", "equity")).unwrap(), ibkr.channel_of(&Instrument::new("AAPL", "equity", "1m")).unwrap());
        assert_eq!(super::super::quote_source("yahoo", "equity", "AAPL"), None, "no live feed at all");
    }

    /// Which timeframes the provider delimits itself. Everything from a day up is a session
    /// (a venue's midnight, a Monday), everything under it is an epoch bucket on every venue
    /// we stream.
    /// A push source has no socket and no credential: a chart subscribes, a posted tick goes
    /// through each subscription's fold and reaches its viewers as the forming candle of every
    /// timeframe on screen, and posted bars reach the subscription at their own timeframe.
    #[tokio::test]
    async fn pushed_ticks_and_bars_reach_the_charts_watching_them() {
        let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
        let hub = LiveHub::new(pool, Client::new(), SecretCipher::from_master("t").unwrap());
        let provider = "push-0000feed";
        crate::histdata::kinds::register(
            provider,
            "Script",
            &serde_json::json!({ "timeframes": "1m, 1h", "columns": "{\"ts\": 0, \"close\": 1}", "ts_unit": "s" }),
        );
        let key = Uuid::new_v4();
        let mut minute = target(provider, key, "ACME", "1m");
        minute.asset_type = "other".into();
        let hour = LiveTarget { timeframe: "1h".into(), ..minute.clone() };
        let mut m = hub.subscribe(minute.clone()).unwrap();
        let mut h = hub.subscribe(hour).unwrap();

        // Ticks inside the minute in progress, so the fold has nothing to close yet.
        let start = super::super::bar_start(OffsetDateTime::now_utc(), 60);
        assert_eq!(hub.ingest(key, "ACME", "other", Ingest::Tick { price: 10.0, size: 2.0, ts: start }), 2);
        assert_eq!(
            hub.ingest(key, "ACME", "other", Ingest::Tick { price: 12.0, size: 1.0, ts: start + TimeDuration::seconds(1) }),
            2
        );
        // Another symbol, or another connector, reaches nobody.
        assert_eq!(hub.ingest(key, "OTHER", "other", Ingest::Tick { price: 1.0, size: 1.0, ts: start }), 0);
        assert_eq!(hub.ingest(Uuid::new_v4(), "ACME", "other", Ingest::Tick { price: 1.0, size: 1.0, ts: start }), 0);

        async fn forming(rx: &mut broadcast::Receiver<LiveMsg>, close: f64) -> LiveEvent {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                match tokio::time::timeout(left, rx.recv()).await {
                    Ok(Ok(LiveMsg::Bar(e))) if e.close == close => return e,
                    Ok(Ok(_)) => continue,
                    other => panic!("no bar closing at {close}: {other:?}"),
                }
            }
        }
        let e = forming(&mut m.rx, 12.0).await;
        assert!(!e.closed);
        assert_eq!((e.bar_ts, e.open, e.high, e.low, e.volume), (start, 10.0, 12.0, 10.0, 3.0));
        let e = forming(&mut h.rx, 12.0).await;
        assert_eq!((e.bar_ts, e.open, e.volume), (super::super::bar_start(start, 3600), 10.0, 3.0));

        // Bars reach the subscription at their timeframe only, the newest forming.
        let bar = |ts: OffsetDateTime, close: f64| Bar {
            ts,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        };
        let hour_start = super::super::bar_start(start, 3600);
        let bars = vec![bar(hour_start - TimeDuration::hours(1), 20.0), bar(hour_start, 21.0)];
        assert_eq!(hub.ingest(key, "ACME", "other", Ingest::Bars { timeframe: "1h".into(), bars }), 1);
        assert!(forming(&mut h.rx, 20.0).await.closed);
        assert!(!forming(&mut h.rx, 21.0).await.closed);
        crate::histdata::kinds::forget(provider);
    }

    /// A pushed forming candle is the period's state: a tick posted after it extends it (the
    /// pushed open and low kept) instead of starting a fresh candle at the tick, and a coarser
    /// pane folds the pushed bars of its hour in as partial bars.
    #[tokio::test]
    async fn a_pushed_forming_bar_primes_the_fold() {
        let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
        let hub = LiveHub::new(pool, Client::new(), SecretCipher::from_master("t").unwrap());
        let provider = "push-0000f0e1";
        crate::histdata::kinds::register(
            provider,
            "Script",
            &serde_json::json!({ "timeframes": "1m, 1h", "columns": "{\"ts\": 0, \"close\": 1}", "ts_unit": "s" }),
        );
        let key = Uuid::new_v4();
        let mut minute = target(provider, key, "ACME", "1m");
        minute.asset_type = "other".into();
        let hour = LiveTarget { timeframe: "1h".into(), ..minute.clone() };
        let mut m = hub.subscribe(minute).unwrap();
        let mut h = hub.subscribe(hour).unwrap();

        // Clear of a minute boundary, so the tick lands in the pushed forming minute.
        if OffsetDateTime::now_utc().second() >= 55 {
            tokio::time::sleep(Duration::from_secs(6)).await;
        }
        let start = super::super::bar_start(OffsetDateTime::now_utc(), 60);
        let bar = |ts: OffsetDateTime, o: f64, hi: f64, lo: f64, c: f64, v: f64| Bar {
            ts,
            open: o,
            high: hi,
            low: lo,
            close: c,
            volume: v,
            adj_open: None,
            adj_high: None,
            adj_low: None,
            adj_close: None,
        };
        let closed = [start - TimeDuration::minutes(2), start - TimeDuration::minutes(1)];
        let mut bars: Vec<Bar> = closed.iter().map(|t| bar(*t, 11.5, 11.5, 11.5, 11.5, 1.0)).collect();
        bars.push(bar(start, 12.0, 12.2, 11.9, 12.1, 5.0));
        assert_eq!(hub.ingest(key, "ACME", "other", Ingest::Bars { timeframe: "1m".into(), bars }), 2, "the minute pane and the hour above it");
        assert_eq!(hub.ingest(key, "ACME", "other", Ingest::Tick { price: 12.4, size: 3.0, ts: start + TimeDuration::seconds(1) }), 2);

        async fn closing(rx: &mut broadcast::Receiver<LiveMsg>, close: f64) -> LiveEvent {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            loop {
                let left = deadline.saturating_duration_since(tokio::time::Instant::now());
                match tokio::time::timeout(left, rx.recv()).await {
                    Ok(Ok(LiveMsg::Bar(e))) if e.close == close => return e,
                    Ok(Ok(_)) => continue,
                    other => panic!("no bar closing at {close}: {other:?}"),
                }
            }
        }
        let e = closing(&mut m.rx, 12.4).await;
        assert!(!e.closed);
        assert_eq!((e.bar_ts, e.open, e.high, e.low, e.close, e.volume), (start, 12.0, 12.4, 11.9, 12.4, 8.0));

        // The hour holds the pushed minutes of this hour, then the tick.
        let hour_start = super::super::bar_start(start, 3600);
        let earlier = closed.iter().filter(|t| **t >= hour_start).count() as f64;
        let e = closing(&mut h.rx, 12.4).await;
        assert_eq!(e.bar_ts, hour_start);
        assert_eq!(e.open, if earlier > 0.0 { 11.5 } else { 12.0 });
        assert_eq!(e.low, if earlier > 0.0 { 11.5 } else { 11.9 });
        assert_eq!((e.high, e.volume), (12.4, earlier + 5.0 + 3.0));

        // Posting the same batch again adds nothing to the hour.
        let again = vec![bar(start, 12.0, 12.2, 11.9, 12.1, 5.0)];
        hub.ingest(key, "ACME", "other", Ingest::Bars { timeframe: "1m".into(), bars: again });
        assert_eq!(hub.ingest(key, "ACME", "other", Ingest::Tick { price: 12.5, size: 1.0, ts: start + TimeDuration::seconds(2) }), 2);
        let e = closing(&mut h.rx, 12.5).await;
        assert_eq!(e.volume, earlier + 5.0 + 3.0 + 1.0);
        crate::histdata::kinds::forget(provider);
    }

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

    /// A gap found while a backfill is running, or one whose request failed, stays owed and
    /// goes out at the next closed bar. Both used to be dropped: the hole stayed stored and the
    /// chart drew the bars either side of it as neighbours.
    #[tokio::test]
    async fn an_unsent_or_failed_gap_backfill_stays_owed() {
        let inner = Arc::new(Inner {
            pool: PgPool::connect_lazy("postgres://unused/unused").unwrap(),
            http: Client::new(),
            cipher: SecretCipher::from_master("t").unwrap(),
            sessions: Mutex::new(HashMap::new()),
            next_gen: Mutex::new(0),
            anchors: Mutex::new(HashMap::new()),
            points: broadcast::channel(4).0,
        });
        let t0 = time::macros::datetime!(2026-10-22 07:07:00 UTC);
        let t1 = time::macros::datetime!(2026-10-22 10:58:00 UTC);

        // Busy: the span is kept, widened by the next one, and nothing is sent.
        let mut run = run_of("coinbase", "crypto", "BTC-USD", "1m");
        run.target.provider = "no_such_provider".into();
        let meta = run.target.clone();
        run.backfilling.store(true, std::sync::atomic::Ordering::SeqCst);
        owe_gap(&run.gap_pending, t0, t1);
        send_owed_gap(&inner, &HashMap::new(), &meta, &run);
        owe_gap(&run.gap_pending, t1, t1 + TimeDuration::minutes(5));
        assert_eq!(*run.gap_pending.lock().unwrap(), Some((t0, t1 + TimeDuration::minutes(5))));

        // Free, and the request fails: the span comes back for the next try.
        run.backfilling.store(false, std::sync::atomic::Ordering::SeqCst);
        send_owed_gap(&inner, &HashMap::new(), &meta, &run);
        for _ in 0..100 {
            if !run.backfilling.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(*run.gap_pending.lock().unwrap(), Some((t0, t1 + TimeDuration::minutes(5))));
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

    /// An hour folded from trades, joined at 10:40: the stored and drawn hour is the venue's
    /// from 10:00, not the forty minutes' worth of nothing before the socket opened. The seed's
    /// running bar is adopted, the fold since connect it replaced is not replayed, and the
    /// viewer gets that bar as forming.
    #[tokio::test]
    async fn a_fold_adopts_the_period_in_progress_from_the_seed() {
        let inner = Arc::new(Inner {
            pool: PgPool::connect_lazy("postgres://unused/unused").unwrap(),
            http: Client::new(),
            cipher: SecretCipher::from_master("t").unwrap(),
            sessions: Mutex::new(HashMap::new()),
            next_gen: Mutex::new(0),
            anchors: Mutex::new(HashMap::new()),
            points: broadcast::channel(4).0,
        });
        let bar = |ts: OffsetDateTime, o: f64, h: f64, l: f64, c: f64, v: f64| Bar {
            ts, open: o, high: h, low: l, close: c, volume: v, adj_open: None, adj_high: None, adj_low: None, adj_close: None,
        };
        let mut run = run_of("coinbase", "crypto", "BTC-USD", "1h");
        assert!(run.agg.is_some(), "coinbase hours are folded from trades");
        let mut rx = run.tx.subscribe();
        let now = OffsetDateTime::now_utc();
        let hour = super::super::bar_start(now, 3600);
        let prev = hour - TimeDuration::hours(1);
        // The fold since connect, held while the seed was in flight.
        run.seeding = true;
        run.buffer = run.agg.as_mut().unwrap().trade(105.0, 1.0, now);
        let key = run.inst.key();
        let meta = run.target.clone();
        let mut subs = HashMap::from([(key.clone(), run)]);
        let seed = vec![bar(prev, 90.0, 95.0, 85.0, 92.0, 70.0), bar(hour, 100.0, 110.0, 90.0, 104.0, 50.0)];
        finish_seed(&inner, &HashMap::new(), &meta, &mut subs, key.clone(), Ok(seed), None).await;

        let run = subs.get_mut(&key).unwrap();
        assert_eq!(run.last_closed, Some(prev), "the running bar is not a closed one");
        let next = run.agg.as_mut().unwrap().trade(111.0, 2.0, now);
        let e = next.last().unwrap();
        assert_eq!((e.bar_ts, e.open, e.high, e.low, e.close, e.volume, e.closed), (hour, 100.0, 111.0, 90.0, 111.0, 52.0, false));

        let mut sent = Vec::new();
        while let Ok(LiveMsg::Bar(e)) = rx.try_recv() {
            sent.push((e.bar_ts, e.open, e.closed));
        }
        assert_eq!(sent, vec![(prev, 90.0, true), (hour, 100.0, false)], "no bar from the fold since connect");
    }

    /// The whole path on a real venue: a chart pane and its bid/ask on one socket, each on its
    /// own channel, and the futures quote on a socket of its own. Needs the network
    /// (`OTW_LIVE_TEST=1`) and a scratch database (`OTW_TEST_DATABASE_URL`, migrated here): the
    /// hub reads a connector's credentials before it opens, and a keyless one has none.
    #[tokio::test]
    async fn live_binance_bars_and_quotes_share_the_socket_on_their_own_channels() {
        let (Ok("1"), Ok(url)) = (std::env::var("OTW_LIVE_TEST").as_deref(), std::env::var("OTW_TEST_DATABASE_URL")) else { return };
        let pool = otw_store::connect_and_migrate(&url).await.unwrap();
        let hub = LiveHub::new(pool, Client::new(), SecretCipher::from_master("t").unwrap());
        let key = Uuid::new_v4();
        let bars = hub.subscribe(target("binance", key, "BTCUSDT", "1m")).unwrap();
        let quote = hub.subscribe(target("binance", key, "BTCUSDT", super::super::QUOTE_TF)).unwrap();
        let fut = hub.subscribe(target("binance_futures", key, "BTCUSDT", super::super::QUOTE_TF)).unwrap();
        assert_eq!(hub.0.sessions.lock().unwrap().len(), 2, "spot bars and quote on one socket, the futures quote on another");
        async fn drain(mut rx: broadcast::Receiver<LiveMsg>) -> (usize, usize) {
            let (mut b, mut q) = (0, 0);
            let end = tokio::time::Instant::now() + Duration::from_secs(10);
            while let Ok(m) = tokio::time::timeout_at(end, rx.recv()).await {
                match m {
                    Ok(LiveMsg::Bar(_)) => b += 1,
                    Ok(LiveMsg::Quote(q0)) => {
                        assert!(q0.bid > 0.0 && q0.ask >= q0.bid, "{q0:?}");
                        q += 1
                    }
                    Ok(LiveMsg::Status(s)) => assert!(s.code.is_none(), "{s:?}"),
                    Err(_) => {}
                }
            }
            (b, q)
        }
        let ((b, q0), (b1, q1), (b2, q2)) = tokio::join!(drain(bars.rx), drain(quote.rx), drain(fut.rx));
        assert!(b >= 3 && q0 == 0, "bars only on the bar channel: {b} bars, {q0} quotes");
        assert!(q1 >= 3 && b1 == 0, "quotes only on the quote channel: {q1} quotes, {b1} bars");
        assert!(q2 >= 3 && b2 == 0, "futures quotes: {q2}");
        let held = hub.0.sessions.lock().unwrap().values().flat_map(|e| e.subs.values()).filter(|s| s.target.timeframe == super::super::QUOTE_TF).count();
        assert_eq!(held, 2);
    }

    /// Coinbase minutes are folded from trades. A pane recording BTC-USD opens mid-minute,
    /// closes, and opens again mid-minute on a fresh socket: each first minute starts from the
    /// venue's open (the seed's running bar), not from the first trade after connect, and every
    /// minute the settle re-read reached is stored exactly as the venue's REST bar, the second
    /// pane's joined minute included (Coinbase can publish it minutes late). About ten minutes. Needs `OTW_LIVE_TEST=1` and `OTW_TEST_DATABASE_URL` (a scratch database).
    #[tokio::test]
    async fn live_coinbase_recorded_minutes_survive_a_reconnect() {
        let (Ok("1"), Ok(url)) = (std::env::var("OTW_LIVE_TEST").as_deref(), std::env::var("OTW_TEST_DATABASE_URL")) else { return };
        let pool = otw_store::connect_and_migrate(&url).await.unwrap();
        let dataset = store::upsert_dataset(&pool, "coinbase", "crypto", "BTC-USD", "1m").await.unwrap();
        let hub = LiveHub::new(pool.clone(), Client::new(), SecretCipher::from_master("t").unwrap());
        let mut t = target("coinbase", Uuid::new_v4(), "BTC-USD", "1m");
        t.dataset_id = Some(dataset);
        // Every closed event per minute, in arrival order: the fold's close, then the re-read.
        type Seen = Arc<Mutex<Vec<(usize, LiveEvent)>>>;
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let listen = |sub: Subscription, run: usize| {
            let seen = seen.clone();
            let mut rx = sub.rx;
            let task = tokio::spawn(async move {
                while let Ok(m) = rx.recv().await {
                    if let LiveMsg::Bar(e) = m {
                        if e.closed {
                            seen.lock().unwrap().push((run, e));
                        }
                    }
                }
            });
            (sub.guard, task)
        };
        async fn at_second(s: u8) {
            loop {
                if OffsetDateTime::now_utc().second() == s {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        async fn until(f: impl Fn() -> bool) {
            while !f() {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }

        at_second(20).await;
        let first_a = super::super::bar_start(OffsetDateTime::now_utc(), 60);
        let (guard, task) = listen(hub.subscribe(t.clone()).unwrap(), 0);
        // The seed re-sends closed history: only minutes from the one we joined count.
        until(|| seen.lock().unwrap().iter().filter(|(r, e)| *r == 0 && e.bar_ts >= first_a).count() >= 2).await;
        at_second(5).await;
        drop(guard);
        tokio::time::sleep(TEARDOWN_GRACE + Duration::from_secs(5)).await;
        assert!(hub.0.sessions.lock().unwrap().is_empty(), "the socket is gone");
        task.abort();
        let first_b = super::super::bar_start(OffsetDateTime::now_utc(), 60);
        let (guard_b, task_b) = listen(hub.subscribe(t.clone()).unwrap(), 1);
        until(|| seen.lock().unwrap().iter().filter(|(r, e)| *r == 1 && e.bar_ts >= first_b + TimeDuration::minutes(2)).count() >= 1).await;
        // The last re-read in flight lands, retries included, then nothing else is waited for.
        tokio::time::sleep(SETTLE_DELAY + SETTLE_RETRY * (SETTLE_TRIES - 1) + Duration::from_secs(20)).await;
        drop(guard_b);
        task_b.abort();

        let end = first_b + TimeDuration::minutes(3);
        let rest = crate::histdata::connector_for("coinbase").unwrap();
        let venue = rest.fetch_chunk(&Client::new(), &HashMap::new(), "BTC-USD", "crypto", "1m", first_a, end).await.unwrap().bars;
        let venue: HashMap<OffsetDateTime, Bar> = venue.into_iter().map(|b| (b.ts, b)).collect();
        let stored: HashMap<OffsetDateTime, Bar> = store::read_bars(&pool, dataset, Some(first_a), Some(end), 100).await.unwrap().into_iter().map(|b| (b.ts, b)).collect();
        let seen = seen.lock().unwrap().clone();
        let mut minutes: Vec<OffsetDateTime> = venue.keys().copied().collect();
        minutes.sort();
        let mut failures = Vec::new();
        for m in minutes {
            let v = &venue[&m];
            let events: Vec<&(usize, LiveEvent)> = seen.iter().filter(|(_, e)| e.bar_ts == m).collect();
            let fold = events.first().map(|(_, e)| e);
            let reread = events.len() > 1;
            let s = stored.get(&m);
            println!(
                "{m}  venue O{} H{} L{} C{} V{:.4} | fold {:?} | stored {:?} | re-read {reread}",
                v.open, v.high, v.low, v.close, v.volume,
                fold.map(|e| (e.open, e.high, e.low, e.close, (e.volume * 1e4).round() / 1e4)),
                s.map(|b| (b.open, b.high, b.low, b.close, (b.volume * 1e4).round() / 1e4)),
            );
            // A minute joined mid-way starts from the venue's open.
            if m == first_a || m == first_b {
                if fold.map(|e| e.open) != Some(v.open) {
                    failures.push(format!("{m}: the joined minute opens at {:?}, the venue at {}", fold.map(|e| e.open), v.open));
                }
            }
            // The second pane's joined minute is re-read while it is still open: stored whole.
            if reread || m == first_b {
                let same = s.is_some_and(|b| (b.open, b.high, b.low, b.close) == (v.open, v.high, v.low, v.close) && (b.volume - v.volume).abs() < 1e-6);
                if !same {
                    failures.push(format!("{m}: stored {s:?}, venue {v:?}"));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

}
