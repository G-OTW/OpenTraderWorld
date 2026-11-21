//! Quasi-live candles for a provider with no socket: its REST bars, polled.
//!
//! Yahoo, EODHD, Alpha Vantage, the HTTP-streaming brokers and every custom `http` source
//! download bars but publish nothing live. For them the hub runs this driver where a native
//! provider runs a WebSocket: every `poll_secs` (a connector setting, [`DEFAULT_POLL_SECS`],
//! at least [`MIN_POLL_SECS`]) it reads the last few bars of each subscribed instrument at
//! the subscribed timeframe, compares them with the previous answer ([`PollState::diff`]) and
//! hands the hub what changed: the forming candle when it moved, a closed event for each bar
//! that ended. To the hub this is a provider publishing its own candle ([`SourceGrain::Native`]):
//! seeding, recording, gap backfill, the anchored path for daily and weekly bars and the
//! settle re-read all run unchanged.
//!
//! Requests are spaced by the provider's `min_interval_ms` (shared with the download worker,
//! [`crate::histdata_job::pace`]) and counted against the connector's declared quota; a spent
//! quota pauses the poll until its window rolls over. A rejected key ends the session, an
//! unknown symbol ends that instrument, anything else backs off and retries.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Result};
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_stream::wrappers::ReceiverStream;

use otw_store::histdata::Bar;

use super::{
    FeedCmd, FeedHandle, FeedUpdate, Instrument, LiveError, LiveEvent, LiveFault, LiveSession, SessionRequest, SourceGrain,
    StreamConnector,
};
use crate::histdata::ConfigField;

/// Seconds between two polls when the connector does not say.
pub const DEFAULT_POLL_SECS: u64 = 15;
/// The shortest poll a connector may ask for: below it a free tier is spent in minutes.
pub const MIN_POLL_SECS: u64 = 5;
/// The longest: past an hour the chart is not live in any useful sense.
pub const MAX_POLL_SECS: u64 = 3600;
/// Bars asked for on each poll: the forming one, the one that just closed, and a margin for
/// a source that publishes a bar late.
const POLL_BARS: i64 = 5;
/// The longest a failing instrument waits before it is asked again.
const MAX_BACKOFF: Duration = Duration::from_secs(300);
/// The longest a spent quota pauses the poll before it is read again.
const QUOTA_WAIT_MAX: Duration = Duration::from_secs(3600);

/// The connector setting, offered by every provider polled for live.
pub const POLL_SECS_FIELD: ConfigField = ConfigField {
    name: "poll_secs",
    label: "Live poll interval (s)",
    placeholder: "15",
    kind: "number",
    required: false,
    help: "This source has no live socket, so a live chart asks it for its last bars every \
           this many seconds (at least 5). Each poll is a request against its rate limit.",
};

/// What a polled provider's live reach costs, shown next to the live control.
pub const NOTE: &str = "Polled live feed: the chart asks for the last bars every poll interval \
                        (15 s unless the connector sets poll_secs), so the candle moves at that \
                        pace and every poll is a request against the provider's limit.";

/// A poll interval held to [`MIN_POLL_SECS`] and [`MAX_POLL_SECS`].
pub fn clamp_poll_secs(secs: u64) -> u64 {
    secs.clamp(MIN_POLL_SECS, MAX_POLL_SECS)
}

/// The `poll_secs` setting, checked: absent is the default, a value outside the bounds is held
/// to them, anything that is not a whole number is an error naming the setting.
pub fn poll_secs_setting(cfg: &HashMap<String, String>) -> Result<u64> {
    match cfg.get("poll_secs").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => Ok(DEFAULT_POLL_SECS),
        Some(s) => match s.parse::<u64>() {
            Ok(n) => Ok(clamp_poll_secs(n)),
            Err(_) => bail!("poll_secs must be a whole number of seconds"),
        },
    }
}

/// The poll interval a connector runs at, the default when its setting does not read.
pub fn poll_secs(cfg: &HashMap<String, String>) -> u64 {
    poll_secs_setting(cfg).unwrap_or(DEFAULT_POLL_SECS)
}

// ── The diff ─────────────────────────────────────────────────────────────────

/// What one instrument's poller remembers between two answers.
#[derive(Debug, Default, Clone)]
pub(crate) struct PollState {
    /// Newest bar already handed over as closed.
    closed_upto: Option<OffsetDateTime>,
    /// The forming bar as last handed over, to tell a moved candle from an unchanged one.
    forming: Option<Bar>,
    /// Whether a first answer has set the baseline.
    primed: bool,
}

fn same(a: &Bar, b: &Bar) -> bool {
    a.ts == b.ts && a.open == b.open && a.high == b.high && a.low == b.low && a.close == b.close && a.volume == b.volume
}

fn event(b: &Bar, closed: bool, now: OffsetDateTime) -> LiveEvent {
    LiveEvent {
        bar_ts: b.ts,
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        volume: b.volume,
        closed,
        provider_ts: now,
    }
}

impl PollState {
    /// Forget the forming bar, so the next answer hands it over again: after a failed poll
    /// the hub has said `retrying`, and only an update tells it the feed is back.
    pub fn reset_forming(&mut self) {
        self.forming = None;
    }

    /// Compare one answer with the previous one and say what a native feed would have sent.
    ///
    /// A bar is closed once a later bar is in the answer (the provider has moved on) or its
    /// period ended more than `grace` ago (a quiet market publishes no next bar). The first
    /// answer only sets the baseline: what closed before the subscription is history, which
    /// the hub seeds on its own. After that every bar that closed since the last answer goes
    /// out as closed, oldest first, several when polls were missed, then the forming bar if
    /// it moved. An answer older than what was already handed over (a stale cache, a page out
    /// of order) changes nothing.
    pub fn diff(&mut self, mut bars: Vec<Bar>, tf_secs: i64, grace_secs: i64, now: OffsetDateTime) -> Vec<LiveEvent> {
        bars.sort_by_key(|b| b.ts);
        bars.dedup_by(|later, earlier| {
            if later.ts == earlier.ts {
                *earlier = later.clone();
                true
            } else {
                false
            }
        });
        let tf = time::Duration::seconds(tf_secs);
        let grace = time::Duration::seconds(grace_secs.max(0));
        let n = bars.len();
        let is_closed = |i: usize, b: &Bar| i + 1 < n || b.ts + tf + grace <= now;
        let mut out = Vec::new();
        let newest_closed = bars.iter().enumerate().filter(|(i, b)| is_closed(*i, b)).map(|(_, b)| b.ts).last();
        let forming = bars.last().filter(|b| !is_closed(n - 1, b)).cloned();

        if !self.primed {
            self.primed = true;
            self.closed_upto = newest_closed;
        } else {
            for (i, b) in bars.iter().enumerate() {
                if is_closed(i, b) && self.closed_upto.is_none_or(|c| b.ts > c) {
                    out.push(event(b, true, now));
                    self.closed_upto = Some(b.ts);
                }
            }
        }
        // The forming bar, when it is newer than everything closed and not an older reading
        // of the one already on screen.
        match forming {
            Some(f) if self.closed_upto.is_none_or(|c| f.ts > c) => {
                let stale = self.forming.as_ref().is_some_and(|p| f.ts < p.ts);
                let moved = !self.forming.as_ref().is_some_and(|p| same(p, &f));
                if !stale && moved {
                    out.push(event(&f, false, now));
                    self.forming = Some(f);
                }
            }
            _ => {
                if self.forming.as_ref().is_some_and(|p| self.closed_upto.is_some_and(|c| p.ts <= c)) {
                    self.forming = None;
                }
            }
        }
        out
    }
}

// ── Classifying a failed poll ────────────────────────────────────────────────

/// What a failed REST read means to the live supervisor. A rejected key and an unknown
/// symbol do not fix themselves; a timeout, a 5xx, a rate limit and anything unnamed do.
pub(crate) fn classify(e: &anyhow::Error) -> LiveFault {
    use otw_connector_model::{ConnectorError, ErrorKind};
    if let Some(l) = e.downcast_ref::<LiveError>() {
        return l.fault;
    }
    for cause in e.chain() {
        if let Some(ce) = cause.downcast_ref::<ConnectorError>() {
            return match ce.kind {
                ErrorKind::Auth => LiveFault::Auth,
                ErrorKind::Permission => LiveFault::Entitlement,
                ErrorKind::NotFound => LiveFault::Symbol,
                ErrorKind::Unsupported => LiveFault::Unsupported,
                _ => LiveFault::Transport,
            };
        }
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            return match re.status().map(|s| s.as_u16()) {
                Some(401) | Some(403) => LiveFault::Auth,
                Some(404) => LiveFault::Symbol,
                _ => LiveFault::Transport,
            };
        }
    }
    // A custom source words its refusals itself (`<name> answered 401 ...`).
    let msg = format!("{e:#}").to_ascii_lowercase();
    if msg.contains("answered 401") || msg.contains("answered 403") || msg.contains("credential") {
        return LiveFault::Auth;
    }
    if msg.contains("answered 404") {
        return LiveFault::Symbol;
    }
    if msg.contains("settings incomplete") || msg.contains("maps no token") {
        return LiveFault::Unsupported;
    }
    LiveFault::Transport
}

/// How long a failing instrument waits after `failures` failures in a row: the poll interval,
/// doubled each time, at most [`MAX_BACKOFF`].
pub(crate) fn backoff(poll: Duration, failures: u32) -> Duration {
    poll.saturating_mul(1u32 << failures.min(8)).min(MAX_BACKOFF)
}

// ── The driver ───────────────────────────────────────────────────────────────

/// The live connector of a provider that is polled.
pub struct PollStream {
    provider: String,
}

impl PollStream {
    pub fn new(provider: &str) -> Self {
        PollStream { provider: provider.to_string() }
    }

    fn check(&self, inst: &Instrument) -> Result<()> {
        let rest = crate::histdata::connector_for(&self.provider)
            .map_err(|e| LiveError::err(LiveFault::Unsupported, format!("{e:#}")))?;
        let cap = rest.capability();
        if !cap.timeframes.contains(&inst.timeframe.as_str()) {
            return Err(LiveError::err(LiveFault::Unsupported, format!("{} serves no {} bars", cap.label, inst.timeframe)));
        }
        if !cap.asset_types.contains(&inst.asset_type.as_str()) {
            return Err(LiveError::err(LiveFault::Unsupported, format!("{} serves no {}", cap.label, inst.asset_type)));
        }
        Ok(())
    }
}

/// One instrument on a poller.
struct Watch {
    inst: Instrument,
    channel: String,
    tf_secs: i64,
    state: PollState,
    failures: u32,
    next_at: Instant,
}

/// Everything a poller needs, moved onto its task.
struct Poller {
    provider: String,
    connector: String,
    rest: Box<dyn crate::histdata::Connector>,
    http: reqwest::Client,
    secrets: HashMap<String, String>,
    poll: Duration,
    min_interval: Duration,
    quota: Option<(sqlx::PgPool, String)>,
    tx: mpsc::Sender<Result<FeedUpdate>>,
}

fn channel_of(inst: &Instrument) -> String {
    format!("{}|{}|{}", inst.symbol, inst.asset_type, inst.timeframe)
}

#[async_trait::async_trait]
impl StreamConnector for PollStream {
    fn grain(&self, inst: &Instrument) -> Result<SourceGrain> {
        self.check(inst)?;
        Ok(SourceGrain::Native)
    }

    fn channel_of(&self, inst: &Instrument) -> Result<String> {
        self.check(inst)?;
        Ok(channel_of(inst))
    }

    async fn open(&self, req: SessionRequest<'_>) -> Result<LiveSession> {
        let rest = crate::histdata::connector_for(&self.provider)
            .map_err(|e| LiveError::err(LiveFault::Unsupported, format!("{e:#}")))?;
        // A custom source whose settings no longer read would fail every poll the same way.
        rest.validate_config(req.secrets)
            .map_err(|e| LiveError::err(LiveFault::Unsupported, format!("{}: {e:#}", req.connector)))?;
        let http = crate::histdata::client()?;
        let (tx, rx) = mpsc::channel::<Result<FeedUpdate>>(512);
        let (cmd_tx, cmd_rx) = mpsc::channel::<FeedCmd>(32);
        let poller = Poller {
            provider: self.provider.clone(),
            connector: req.connector.to_string(),
            min_interval: Duration::from_millis(rest.capability().min_interval_ms),
            rest,
            http,
            secrets: req.secrets.clone(),
            poll: Duration::from_secs(poll_secs(req.secrets)),
            quota: req.pool.zip(req.connector_id).map(|(p, id)| (p.clone(), crate::connectors_api::quota_scope(id))),
            tx,
        };
        let opening = req.instruments.to_vec();
        tokio::spawn(poller.run(opening, cmd_rx));
        Ok(LiveSession { handle: FeedHandle::new(cmd_tx), stream: Box::pin(ReceiverStream::new(rx)) })
    }
}

impl Poller {
    fn watch(inst: Instrument) -> Option<Watch> {
        let tf_secs = crate::histdata::timeframe_secs(&inst.timeframe).ok()?;
        Some(Watch { channel: channel_of(&inst), inst, tf_secs, state: PollState::default(), failures: 0, next_at: Instant::now() })
    }

    async fn run(self, opening: Vec<Instrument>, mut cmds: mpsc::Receiver<FeedCmd>) {
        let mut watches: HashMap<String, Watch> = HashMap::new();
        for inst in opening {
            if let Some(w) = Self::watch(inst) {
                watches.insert(w.channel.clone(), w);
            }
        }
        let mut tick = tokio::time::interval(self.poll);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if !self.round(&mut watches).await {
                        return;
                    }
                }
                cmd = cmds.recv() => match cmd {
                    // A joining pane is asked for at once rather than at the next tick.
                    Some(FeedCmd::Add(insts)) => {
                        for inst in insts {
                            if let Some(w) = Self::watch(inst) {
                                watches.entry(w.channel.clone()).or_insert(w);
                            }
                        }
                        if !self.round(&mut watches).await {
                            return;
                        }
                    }
                    Some(FeedCmd::Remove(insts)) => {
                        for inst in insts {
                            watches.remove(&channel_of(&inst));
                        }
                    }
                    None => return,
                },
            }
        }
    }

    /// When the connector's declared quota is spent, the instant its window rolls over.
    async fn quota_spent(&self) -> Option<(OffsetDateTime, String)> {
        let (pool, scope) = self.quota.as_ref()?;
        let q = otw_store::api_quota::get(pool, scope).await.ok().flatten()?;
        let max = q.max_requests?;
        (q.used >= max).then(|| {
            let at = q.resets_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
            (q.resets_at, format!("{}: request limit reached ({}/{max} per {}), polling resumes {at}", self.connector, q.used, q.period))
        })
    }

    /// Poll every instrument that is due. False when the session is over: the hub let go of
    /// it, or the provider refused the connector's credentials.
    async fn round(&self, watches: &mut HashMap<String, Watch>) -> bool {
        let keys: Vec<String> = watches.keys().cloned().collect();
        for key in keys {
            let Some(w) = watches.get(&key) else { continue };
            if w.next_at > Instant::now() {
                continue;
            }
            if let Some((resets, message)) = self.quota_spent().await {
                let wait = (resets - OffsetDateTime::now_utc()).try_into().unwrap_or(QUOTA_WAIT_MAX).min(QUOTA_WAIT_MAX);
                let err = LiveError::err(LiveFault::Transport, message);
                for w in watches.values_mut() {
                    w.next_at = Instant::now() + wait;
                    w.state.reset_forming();
                    if self.tx.send(Ok(FeedUpdate::fault(w.channel.clone(), &err))).await.is_err() {
                        return false;
                    }
                }
                return true;
            }
            crate::histdata_job::pace(&self.provider, self.min_interval).await;
            if let Some((pool, scope)) = &self.quota {
                if let Err(e) = otw_store::api_quota::bump(pool, scope).await {
                    tracing::debug!("live poll: quota bump failed: {e:#}");
                }
            }
            let Some(w) = watches.get_mut(&key) else { continue };
            let now = OffsetDateTime::now_utc();
            let from = now - time::Duration::seconds(w.tf_secs * POLL_BARS);
            let answer = self
                .rest
                .fetch_seed(&self.http, &self.secrets, &w.inst.symbol, &w.inst.asset_type, &w.inst.timeframe, from, now)
                .await;
            match answer {
                Ok(chunk) => {
                    w.failures = 0;
                    w.next_at = Instant::now();
                    let grace = self.poll.as_secs() as i64;
                    for ev in w.state.diff(chunk.bars, w.tf_secs, grace, OffsetDateTime::now_utc()) {
                        if self.tx.send(Ok(FeedUpdate::bar(w.channel.clone(), ev))).await.is_err() {
                            return false;
                        }
                    }
                }
                Err(e) => {
                    let fault = classify(&e);
                    let err = LiveError::err(fault, format!("{}: {}", self.connector, super::fault_message(&e)));
                    if fault == LiveFault::Auth {
                        // One key for every instrument: the session ends and the hub stops.
                        let _ = self.tx.send(Err(err)).await;
                        return false;
                    }
                    if self.tx.send(Ok(FeedUpdate::fault(w.channel.clone(), &err))).await.is_err() {
                        return false;
                    }
                    if fault.terminal() {
                        watches.remove(&key);
                        continue;
                    }
                    w.failures += 1;
                    w.state.reset_forming();
                    let wait = backoff(self.poll, w.failures).max(crate::rate::retry_after(&self.provider).unwrap_or_default());
                    w.next_at = Instant::now() + wait;
                    tracing::warn!("live poll {} {} ({}): {e:#}", w.inst.symbol, w.inst.timeframe, self.connector);
                }
            }
        }
        !self.tx.is_closed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn bar(ts: OffsetDateTime, close: f64) -> Bar {
        Bar { ts, open: 1.0, high: close.max(1.0), low: 1.0, close, volume: 1.0, adj_open: None, adj_high: None, adj_low: None, adj_close: None }
    }

    fn at(m: u8, s: u8) -> OffsetDateTime {
        datetime!(2026-11-06 10:00:00 UTC) + time::Duration::minutes(m as i64) + time::Duration::seconds(s as i64)
    }

    fn summary(evs: &[LiveEvent]) -> Vec<(OffsetDateTime, f64, bool)> {
        evs.iter().map(|e| (e.bar_ts, e.close, e.closed)).collect()
    }

    /// The first answer is a baseline: the forming candle goes out, history does not.
    #[test]
    fn the_first_answer_sends_the_forming_candle_only() {
        let mut s = PollState::default();
        let evs = s.diff(vec![bar(at(0, 0), 1.0), bar(at(1, 0), 2.0), bar(at(2, 0), 3.0)], 60, 15, at(2, 30));
        assert_eq!(summary(&evs), vec![(at(2, 0), 3.0, false)]);
    }

    #[test]
    fn a_moved_forming_candle_is_sent_and_an_unchanged_one_is_not() {
        let mut s = PollState::default();
        s.diff(vec![bar(at(1, 0), 2.0), bar(at(2, 0), 3.0)], 60, 15, at(2, 10));
        // Nothing moved: nothing to say.
        assert!(s.diff(vec![bar(at(1, 0), 2.0), bar(at(2, 0), 3.0)], 60, 15, at(2, 25)).is_empty());
        // The close moved.
        let evs = s.diff(vec![bar(at(1, 0), 2.0), bar(at(2, 0), 3.5)], 60, 15, at(2, 40));
        assert_eq!(summary(&evs), vec![(at(2, 0), 3.5, false)]);
    }

    #[test]
    fn a_new_bar_closes_the_previous_one() {
        let mut s = PollState::default();
        s.diff(vec![bar(at(1, 0), 2.0), bar(at(2, 0), 3.0)], 60, 15, at(2, 50));
        let evs = s.diff(vec![bar(at(1, 0), 2.0), bar(at(2, 0), 3.2), bar(at(3, 0), 4.0)], 60, 15, at(3, 5));
        assert_eq!(summary(&evs), vec![(at(2, 0), 3.2, true), (at(3, 0), 4.0, false)]);
        // Said once.
        assert!(s.diff(vec![bar(at(2, 0), 3.2), bar(at(3, 0), 4.0)], 60, 15, at(3, 20)).is_empty());
    }

    /// A quiet market publishes no next bar: the last one closes on the clock, one poll late.
    #[test]
    fn a_bar_with_no_successor_closes_after_its_period_and_the_grace() {
        let mut s = PollState::default();
        s.diff(vec![bar(at(2, 0), 3.0)], 60, 15, at(2, 30));
        assert!(s.diff(vec![bar(at(2, 0), 3.0)], 60, 15, at(3, 10)).is_empty(), "inside the grace");
        let evs = s.diff(vec![bar(at(2, 0), 3.0)], 60, 15, at(3, 16));
        assert_eq!(summary(&evs), vec![(at(2, 0), 3.0, true)]);
    }

    /// Missed polls (a slow round, a backoff): every bar that closed meanwhile goes out, in order.
    #[test]
    fn a_gap_of_several_bars_sends_each_closed_bar_oldest_first() {
        let mut s = PollState::default();
        s.diff(vec![bar(at(0, 0), 1.0), bar(at(1, 0), 2.0)], 60, 15, at(1, 30));
        let evs = s.diff(
            vec![bar(at(4, 0), 5.0), bar(at(2, 0), 3.0), bar(at(1, 0), 2.1), bar(at(3, 0), 4.0)],
            60,
            15,
            at(4, 20),
        );
        assert_eq!(
            summary(&evs),
            vec![(at(1, 0), 2.1, true), (at(2, 0), 3.0, true), (at(3, 0), 4.0, true), (at(4, 0), 5.0, false)]
        );
    }

    /// A stale answer (a cache, a page out of order) never rewinds what was sent.
    #[test]
    fn an_answer_out_of_order_changes_nothing() {
        let mut s = PollState::default();
        s.diff(vec![bar(at(2, 0), 3.0), bar(at(3, 0), 4.0)], 60, 15, at(3, 10));
        s.diff(vec![bar(at(3, 0), 4.0), bar(at(4, 0), 5.0)], 60, 15, at(4, 10));
        // Older than what was closed and formed: nothing.
        assert!(s.diff(vec![bar(at(2, 0), 9.0), bar(at(3, 0), 9.0)], 60, 15, at(4, 20)).is_empty());
        // Duplicated stamps: the later reading wins, once.
        let evs = s.diff(vec![bar(at(3, 0), 4.0), bar(at(4, 0), 5.0), bar(at(4, 0), 5.5)], 60, 15, at(4, 30));
        assert_eq!(summary(&evs), vec![(at(4, 0), 5.5, false)]);
        assert!(s.diff(vec![], 60, 15, at(4, 40)).is_empty());
    }

    #[test]
    fn after_a_failed_poll_the_forming_candle_is_sent_again() {
        let mut s = PollState::default();
        s.diff(vec![bar(at(2, 0), 3.0)], 60, 15, at(2, 10));
        s.reset_forming();
        let evs = s.diff(vec![bar(at(2, 0), 3.0)], 60, 15, at(2, 40));
        assert_eq!(summary(&evs), vec![(at(2, 0), 3.0, false)]);
    }

    #[test]
    fn the_poll_interval_is_held_to_its_bounds() {
        let cfg = |v: &str| HashMap::from([("poll_secs".to_string(), v.to_string())]);
        assert_eq!(poll_secs_setting(&HashMap::new()).unwrap(), DEFAULT_POLL_SECS);
        assert_eq!(poll_secs_setting(&cfg("")).unwrap(), DEFAULT_POLL_SECS);
        assert_eq!(poll_secs_setting(&cfg("1")).unwrap(), MIN_POLL_SECS);
        assert_eq!(poll_secs_setting(&cfg("0")).unwrap(), MIN_POLL_SECS);
        assert_eq!(poll_secs_setting(&cfg("30")).unwrap(), 30);
        assert_eq!(poll_secs_setting(&cfg("999999")).unwrap(), MAX_POLL_SECS);
        assert!(poll_secs_setting(&cfg("fast")).is_err());
        assert_eq!(poll_secs(&cfg("fast")), DEFAULT_POLL_SECS);
    }

    #[test]
    fn a_failing_instrument_backs_off_and_a_refusal_is_named() {
        let poll = Duration::from_secs(15);
        assert_eq!(backoff(poll, 1), Duration::from_secs(30));
        assert_eq!(backoff(poll, 2), Duration::from_secs(60));
        assert_eq!(backoff(poll, 20), MAX_BACKOFF);
        assert_eq!(classify(&anyhow::anyhow!("Feed answered 401 Unauthorized: bad key")), LiveFault::Auth);
        assert_eq!(classify(&anyhow::anyhow!("Feed answered 404 Not Found: no such symbol")), LiveFault::Symbol);
        assert_eq!(classify(&anyhow::anyhow!("connection reset")), LiveFault::Transport);
        assert_eq!(classify(&LiveError::err(LiveFault::Entitlement, "x")), LiveFault::Entitlement);
    }
}
