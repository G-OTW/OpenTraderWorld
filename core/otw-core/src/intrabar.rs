//! Lower-timeframe candles under the bars where the order of events matters, loaded on demand.
//!
//! The engine asks a [`Lower`] for the candles under one strategy bar only when that bar is
//! ambiguous: the stop and a target both reached, or a limit filled mid-bar. A run therefore
//! never loads a lower timeframe it does not need. The answer comes from, in order:
//!
//! 1. a stored dataset of the same instrument (same provider) at the lower timeframe;
//! 2. the intrabar cache, windows fetched by an earlier run;
//! 3. the provider, only when the run is allowed to download (the user's checkbox). A fetched
//!    window lands in the cache, never in a dataset: it is scattered by nature.
//!
//! What could not be found is recorded per bar, so the caller can tell the user how many bars
//! settled by the worst case and what fetching them would cost.
//!
//! A bar's **bid/ask** is loaded the same way ([`QuoteFeed`]): asked only for a bar where a fill
//! can happen, read from the dataset's own quote columns, and fetched when allowed, the easiest
//! way the provider offers: a page of bid/ask candles (Capital.com, OANDA, IBKR, forex.com), or
//! the quotes in force at the bar's open and close (Alpaca, Massive).

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::connectors as conn_store;
use otw_store::histdata::{self as hd, IntrabarBar, IntrabarKey};

use crate::backtest::{Execution, Quote, QuoteSource, SubSlice, SubSource};
use crate::histdata::QuoteHistory;
use crate::AppState;

/// Lower timeframes a bar can be read on, finest first.
pub const SUB_TIMEFRAMES: [&str; 5] = ["1m", "5m", "15m", "1h", "4h"];

/// A window ending this recently may not be fully published yet: it is asked again for a moment,
/// and an incomplete one is not cached, so the next run asks again.
const PUBLISH_WAIT: Duration = Duration::from_millis(700);
const PUBLISH_TRIES: usize = 4;

/// Shared by every asset of one run: what it may do and how far it got.
pub struct Ctx {
    state: AppState,
    handle: tokio::runtime::Handle,
    /// Leave to download lower-timeframe candles (`execution.intrabar_fetch`).
    fetch: bool,
    /// Leave to download bid/ask (`execution.use_quotes`: asking for bid/ask is asking for it).
    fetch_quotes: bool,
    /// Downloads this run may make (a paper tick never seeds a whole history at once).
    max_fetch: usize,
    use_quotes: bool,
    pub progress: Arc<Progress>,
}

/// Live counters a download job reports while the run goes.
#[derive(Default)]
pub struct Progress {
    /// Windows downloaded.
    pub fetched: AtomicUsize,
    /// Windows the provider refused or failed on.
    pub failed: AtomicUsize,
    /// Provider requests sent.
    pub requests: AtomicUsize,
    pub cancel: AtomicBool,
    /// Last provider error, for the report.
    pub last_error: Mutex<Option<String>>,
}

/// One asset's lower-timeframe source.
pub struct Lower {
    ctx: Arc<Ctx>,
    key: IntrabarKey,
    stored: Option<Uuid>,
    parent: Vec<OffsetDateTime>,
    parent_secs: i64,
    sub_secs: i64,
    /// Provider requests one window costs (a window wider than one page needs several).
    per_window: usize,
    min_interval: Duration,
    rate_limit: &'static str,
    memo: Mutex<HashMap<usize, Option<Arc<SubSlice>>>>,
    missing: Mutex<BTreeSet<usize>>,
    breaker: Breaker,
}

/// Downloads from one source stop for the rest of the run after a refusal that will not change
/// within it (a permission, a subscription, a key, a rate limit) or after this many failures in
/// a row: every further bar would spend a request on the same answer.
const GIVE_UP_AFTER: usize = 3;

#[derive(Default)]
struct Breaker {
    failures: AtomicUsize,
    stopped: Mutex<Option<String>>,
}

impl Breaker {
    fn open(&self) -> bool {
        self.stopped.lock().map(|s| s.is_some()).unwrap_or(false)
    }
    fn ok(&self) {
        self.failures.store(0, Ordering::Relaxed);
    }
    fn failed(&self, e: &anyhow::Error) {
        let msg = format!("{e:#}");
        let lower = msg.to_lowercase();
        let refused = crate::histdata_job::is_rate_limited(e)
            || ["permission", "subscription", "unauthori", "forbidden", "401", "403", "api key", "quota"]
                .iter()
                .any(|w| lower.contains(w));
        let n = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if refused || n >= GIVE_UP_AFTER {
            if let Ok(mut s) = self.stopped.lock() {
                s.get_or_insert(msg);
            }
        }
    }
    fn reason(&self) -> Option<String> {
        self.stopped.lock().ok().and_then(|s| s.clone())
    }
}

/// What a run's lower timeframes could not find, and what fetching it would cost.
pub struct Intrabar {
    pub ctx: Arc<Ctx>,
    pub lowers: Vec<Arc<Lower>>,
    pub quotes: Vec<Arc<QuoteFeed>>,
}

/// The box an [`crate::backtest::OwnedBars`] holds; the caller keeps the `Arc` to read the
/// misses after the run.
struct Shared(Arc<Lower>);

impl SubSource for Shared {
    fn candles(&self, bar: usize) -> Option<Arc<SubSlice>> {
        self.0.candles(bar)
    }
    fn timeframe(&self) -> Option<String> {
        Some(self.0.key.timeframe.clone())
    }
}

/// Leave to download: up to `max` windows, counted on `progress`.
pub struct Fetch {
    pub max: usize,
    pub progress: Arc<Progress>,
}

impl Ctx {
    pub fn new(state: &AppState, ex: &Execution, fetch: Option<Fetch>) -> Arc<Ctx> {
        let (allowed, max_fetch, progress) = match fetch {
            Some(f) => (true, f.max, f.progress),
            None => (false, 0, Arc::new(Progress::default())),
        };
        Arc::new(Ctx {
            state: state.clone(),
            handle: tokio::runtime::Handle::current(),
            fetch: allowed && ex.intrabar_fetch,
            fetch_quotes: allowed && ex.use_quotes,
            max_fetch,
            use_quotes: ex.use_quotes,
            progress,
        })
    }
}

/// A run with downloads, held in memory while the page polls it. The newest few are kept.
pub struct Job {
    pub id: Uuid,
    pub progress: Arc<Progress>,
    outcome: Mutex<Option<Result<Value, String>>>,
}

static JOBS: Mutex<Vec<Arc<Job>>> = Mutex::new(Vec::new());
const JOBS_KEPT: usize = 16;

impl Job {
    pub fn register() -> Arc<Job> {
        let job = Arc::new(Job { id: Uuid::new_v4(), progress: Arc::new(Progress::default()), outcome: Mutex::new(None) });
        if let Ok(mut all) = JOBS.lock() {
            all.push(job.clone());
            let extra = all.len().saturating_sub(JOBS_KEPT);
            all.drain(..extra);
        }
        job
    }

    pub fn find(id: Uuid) -> Option<Arc<Job>> {
        JOBS.lock().ok()?.iter().find(|j| j.id == id).cloned()
    }

    pub fn finish(&self, out: Result<Value, String>) {
        if let Ok(mut o) = self.outcome.lock() {
            *o = Some(out);
        }
    }

    pub fn status(&self) -> Value {
        let p = &self.progress;
        let outcome = self.outcome.lock().ok().and_then(|o| o.clone());
        json!({
            "done": outcome.is_some(),
            "fetched": p.fetched.load(Ordering::Relaxed),
            "failed": p.failed.load(Ordering::Relaxed),
            "requests": p.requests.load(Ordering::Relaxed),
            "cancelled": p.cancel.load(Ordering::Relaxed),
            "last_error": p.last_error.lock().ok().and_then(|e| e.clone()),
            "result": outcome.as_ref().and_then(|o| o.as_ref().ok()),
            "error": outcome.as_ref().and_then(|o| o.as_ref().err()),
        })
    }
}

impl Lower {
    /// The source for dataset `ds` whose bars are stamped `parent`, or why there is none.
    pub async fn new(
        ctx: &Arc<Ctx>,
        ds: &hd::Dataset,
        parent: Vec<OffsetDateTime>,
        ex: &Execution,
    ) -> Result<Lower, String> {
        let parent_secs = crate::histdata::timeframe_secs(&ds.timeframe).unwrap_or(0);
        let lower = |tf: &str| crate::histdata::timeframe_secs(tf).is_ok_and(|s| s < parent_secs);
        let candidates: Vec<&str> = if ex.intrabar_timeframe.is_empty() {
            SUB_TIMEFRAMES.into_iter().filter(|t| lower(t)).collect()
        } else {
            [ex.intrabar_timeframe.as_str()].into_iter().filter(|t| lower(t)).collect()
        };
        if candidates.is_empty() {
            return Err(format!("no timeframe below {} to read", ds.timeframe));
        }
        let cap = crate::histdata::capabilities().into_iter().find(|c| c.provider == ds.provider);
        // A stored lower dataset wins: it costs nothing to read.
        let mut chosen: Option<(&str, Option<Uuid>)> = None;
        for tf in &candidates {
            if let Ok(Some(sub)) =
                hd::find_dataset(&ctx.state.pool, &ds.provider, &ds.asset_type, &ds.ticker, tf).await
            {
                chosen = Some((tf, Some(sub.id)));
                break;
            }
        }
        // Otherwise the finest the provider serves with one window per request.
        if chosen.is_none() {
            let served = |tf: &str| cap.is_some_and(|c| c.timeframes.contains(&tf));
            let fits = |tf: &str| {
                let secs = crate::histdata::timeframe_secs(tf).unwrap_or(1).max(1);
                cap.is_none_or(|c| (parent_secs / secs) as u32 <= c.max_bars_per_req.max(1))
            };
            chosen = candidates
                .iter()
                .find(|tf| served(tf) && (fits(tf) || !ex.intrabar_timeframe.is_empty()))
                .map(|tf| (*tf, None));
        }
        let Some((tf, stored)) = chosen else {
            return Err(format!("{} serves no timeframe below {}", ds.provider, ds.timeframe));
        };
        let sub_secs = crate::histdata::timeframe_secs(tf).unwrap_or(60).max(1);
        let page = cap.map(|c| c.max_bars_per_req.max(1) as i64).unwrap_or(1000);
        let per_window = ((parent_secs / sub_secs + page - 1) / page).max(1) as usize;
        Ok(Lower {
            ctx: ctx.clone(),
            key: IntrabarKey {
                provider: ds.provider.clone(),
                asset_type: ds.asset_type.clone(),
                ticker: ds.ticker.clone(),
                timeframe: tf.to_string(),
            },
            stored,
            parent,
            parent_secs,
            sub_secs,
            per_window,
            min_interval: Duration::from_millis(cap.map(|c| c.min_interval_ms).unwrap_or(0)),
            rate_limit: cap.map(|c| c.rate_limit).unwrap_or(""),
            memo: Mutex::new(HashMap::new()),
            missing: Mutex::new(BTreeSet::new()),
            breaker: Breaker::default(),
        })
    }

    pub fn boxed(self: &Arc<Self>) -> Box<dyn SubSource + Send + Sync> {
        Box::new(Shared(self.clone()))
    }

    fn window(&self, bar: usize) -> Option<(OffsetDateTime, OffsetDateTime)> {
        let from = *self.parent.get(bar)?;
        let to = self
            .parent
            .get(bar + 1)
            .copied()
            .unwrap_or(from + time::Duration::seconds(self.parent_secs));
        Some((from, to))
    }

    fn candles(&self, bar: usize) -> Option<Arc<SubSlice>> {
        if let Some(hit) = self.memo.lock().ok()?.get(&bar) {
            return hit.clone();
        }
        let (from, to) = self.window(bar)?;
        // The engine runs on a blocking thread or inside a request; either way the lookup is
        // async, so it is driven to completion here without stalling the runtime.
        let handle = self.ctx.handle.clone();
        let got = tokio::task::block_in_place(|| handle.block_on(self.load(from, to)));
        let slice = got.filter(|b| !b.is_empty()).map(|bars| Arc::new(self.slice(bars)));
        if slice.is_none() {
            if let Ok(mut m) = self.missing.lock() {
                m.insert(bar);
            }
        }
        if let Ok(mut m) = self.memo.lock() {
            m.insert(bar, slice.clone());
        }
        slice
    }

    fn slice(&self, bars: Vec<IntrabarBar>) -> SubSlice {
        let quoted = self.ctx.use_quotes && bars.iter().any(|b| b.quote.is_some());
        SubSlice {
            open: bars.iter().map(|b| b.open).collect(),
            high: bars.iter().map(|b| b.high).collect(),
            low: bars.iter().map(|b| b.low).collect(),
            quotes: quoted.then(|| bars.iter().map(|b| b.quote).collect()),
        }
    }

    /// Stored dataset, then cache, then the provider. None = nothing to read.
    async fn load(&self, from: OffsetDateTime, to: OffsetDateTime) -> Option<Vec<IntrabarBar>> {
        let pool = &self.ctx.state.pool;
        let last = to - time::Duration::seconds(1);
        if let Some(id) = self.stored {
            let rows = hd::read_bars(pool, id, Some(from), Some(last), 100_000).await.ok()?;
            if !rows.is_empty() {
                let quotes: HashMap<OffsetDateTime, Quote> = if self.ctx.use_quotes {
                    hd::read_quotes(pool, id, from, last)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|q| (q.ts, q.quote))
                        .collect()
                } else {
                    HashMap::new()
                };
                return Some(
                    rows.into_iter()
                        .map(|b| IntrabarBar {
                            ts: b.ts,
                            open: b.open,
                            high: b.high,
                            low: b.low,
                            close: b.close,
                            quote: quotes.get(&b.ts).copied(),
                        })
                        .collect(),
                );
            }
        }
        if hd::intrabar_window_known(pool, &self.key, from).await.unwrap_or(false) {
            return hd::read_intrabar(pool, &self.key, from, to).await.ok();
        }
        let p = &self.ctx.progress;
        if !self.ctx.fetch
            || self.breaker.open()
            || p.cancel.load(Ordering::Relaxed)
            || p.fetched.load(Ordering::Relaxed) >= self.ctx.max_fetch
        {
            return None;
        }
        match self.fetch(from, to).await {
            Ok(bars) => {
                self.breaker.ok();
                p.fetched.fetch_add(1, Ordering::Relaxed);
                Some(bars)
            }
            Err(e) => {
                self.breaker.failed(&e);
                p.failed.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("intrabar: {} {} {}: {e:#}", self.key.provider, self.key.ticker, self.key.timeframe);
                if let Ok(mut last) = p.last_error.lock() {
                    *last = Some(format!("{e:#}"));
                }
                None
            }
        }
    }

    /// Download one window, page by page, waiting a moment for a window that just closed.
    async fn fetch(&self, from: OffsetDateTime, to: OffsetDateTime) -> anyhow::Result<Vec<IntrabarBar>> {
        let state = &self.ctx.state;
        let connector = crate::histdata::connector_for(&self.key.provider)?;
        let conn = conn_store::default_for(&state.pool, &self.key.provider).await?;
        let secrets = match &conn {
            Some(c) => conn_store::load_creds(&state.pool, &state.cipher, c.id).await?,
            None => Default::default(),
        };
        let last_needed = to - time::Duration::seconds(self.sub_secs);
        let recent = to > OffsetDateTime::now_utc() - time::Duration::seconds(self.sub_secs * 2);
        let tries = if recent { PUBLISH_TRIES } else { 1 };
        let mut bars: Vec<IntrabarBar> = Vec::new();
        for attempt in 0..tries {
            if attempt > 0 {
                tokio::time::sleep(PUBLISH_WAIT).await;
            }
            bars.clear();
            let mut cursor = from;
            for _ in 0..self.per_window.max(1) + 1 {
                if cursor >= to {
                    break;
                }
                let chunk = self.request(&*connector, conn.as_ref().map(|c| c.id), &secrets, cursor, to).await?;
                let mut got = 0;
                let quotes: HashMap<OffsetDateTime, Quote> =
                    chunk.quotes.into_iter().map(|q| (q.ts, q.quote)).collect();
                for b in chunk.bars.into_iter().filter(|b| b.ts >= cursor && b.ts < to) {
                    got += 1;
                    let quote = quotes.get(&b.ts).copied();
                    bars.push(IntrabarBar { ts: b.ts, open: b.open, high: b.high, low: b.low, close: b.close, quote });
                }
                match bars.last() {
                    Some(b) if got > 0 && b.ts < last_needed => cursor = b.ts + time::Duration::seconds(self.sub_secs),
                    _ => break,
                }
            }
            bars.sort_by_key(|b| b.ts);
            bars.dedup_by_key(|b| b.ts);
            if bars.last().is_some_and(|b| b.ts >= last_needed) {
                break;
            }
        }
        let complete = bars.last().is_some_and(|b| b.ts >= last_needed);
        // A window that just closed and is not all there yet stays uncached: the next run asks.
        if complete || !recent {
            hd::write_intrabar(&state.pool, &self.key, from, to, &bars).await?;
        }
        Ok(bars)
    }

    /// One paced, counted provider request, with one retry on a transient failure.
    async fn request(
        &self,
        connector: &dyn crate::histdata::Connector,
        conn: Option<Uuid>,
        secrets: &HashMap<String, String>,
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> anyhow::Result<crate::histdata::Chunk> {
        let state = &self.ctx.state;
        let mut attempt = 0;
        loop {
            crate::histdata_job::pace(&self.key.provider, self.min_interval).await;
            if let Some(id) = conn {
                let _ = otw_store::api_quota::bump(&state.pool, &crate::connectors_api::quota_scope(id)).await;
            }
            self.ctx.progress.requests.fetch_add(1, Ordering::Relaxed);
            match connector
                .fetch_chunk(&state.http, secrets, &self.key.ticker, &self.key.asset_type, &self.key.timeframe, from, to)
                .await
            {
                Ok(c) => return Ok(c),
                Err(e) if attempt == 0 && crate::histdata_job::is_transient(&e) && !crate::histdata_job::is_rate_limited(&e) => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub fn missing(&self) -> usize {
        self.missing.lock().map(|m| m.len()).unwrap_or(0)
    }
}

impl Intrabar {
    /// The sources whose downloads stopped during the run, and why: what the result has to say
    /// next to numbers that fell back to the worst case or the spread.
    pub fn notes(&self) -> Vec<String> {
        let lower = self.lowers.iter().filter_map(|l| {
            l.breaker.reason().map(|why| {
                format!("{} {}: lower-timeframe download stopped ({why}), unresolved bars count as a stop", l.key.ticker, l.key.timeframe)
            })
        });
        let quotes = self.quotes.iter().filter_map(|q| {
            q.breaker.reason().map(|why| format!("{}: bid/ask download stopped ({why}), fills use the spread", q.key.ticker))
        });
        lower.chain(quotes).collect()
    }

    /// Bars that settled by the worst case for want of lower-timeframe candles.
    pub fn missing(&self) -> usize {
        self.lowers.iter().map(|l| l.missing()).sum()
    }

    /// What fetching the missing bars would cost: requests, seconds at the provider's pace, its
    /// published limit and the quota left on its connector. Null when nothing is missing.
    pub async fn estimate(&self) -> Value {
        let missing = self.missing();
        let missing_quotes: usize = self.quotes.iter().map(|q| q.missing().len()).sum();
        if missing == 0 && missing_quotes == 0 {
            return Value::Null;
        }
        let mut requests = 0usize;
        let mut seconds = 0f64;
        let mut providers = Vec::new();
        let rows = self
            .lowers
            .iter()
            .map(|l| ("intrabar", &l.key, l.missing(), l.missing() * l.per_window, l.min_interval, l.rate_limit))
            .chain(self.quotes.iter().map(|q| {
                let n = q.missing().len();
                ("quotes", &q.key, n, q.requests_for(&q.missing()), q.min_interval, q.rate_limit)
            }));
        for (kind, key, n, r, pace, rate_limit) in rows {
            if n == 0 {
                continue;
            }
            requests += r;
            // The pace the worker keeps, or a conservative round trip when it keeps none.
            seconds += r as f64 * pace.as_secs_f64().max(0.5);
            providers.push(json!({
                "kind": kind,
                "ticker": key.ticker,
                "provider": key.provider,
                "timeframe": key.timeframe,
                "bars": n,
                "requests": r,
                "rate_limit": rate_limit,
                "quota": quota_of(&self.ctx.state, &key.provider).await,
            }));
        }
        json!({
            "missing_bars": missing,
            "missing_quotes": missing_quotes,
            "requests": requests,
            "seconds": seconds.ceil() as u64,
            "providers": providers,
        })
    }
}

/// The quota declared on a provider's default connector, as the estimate shows it.
async fn quota_of(state: &AppState, provider: &str) -> Value {
    match conn_store::default_for(&state.pool, provider).await {
        Ok(Some(c)) => otw_store::api_quota::get(&state.pool, &crate::connectors_api::quota_scope(c.id))
            .await
            .ok()
            .flatten()
            .map(|q| json!({ "used": q.used, "max": q.max_requests, "period": q.period, "resets_at": q.resets_at.unix_timestamp() }))
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

// ── Bid/ask on demand ─────────────────────────────────────────────────────────

/// One asset's bid/ask source: what the dataset stores, then the provider when allowed.
pub struct QuoteFeed {
    ctx: Arc<Ctx>,
    key: IntrabarKey,
    dataset: Uuid,
    kind: QuoteHistory,
    parent: Vec<OffsetDateTime>,
    parent_secs: i64,
    /// The bars' own high and low, which a quote read at the open and the close extends.
    high: Vec<f64>,
    low: Vec<f64>,
    stored: Vec<Option<Quote>>,
    page: usize,
    min_interval: Duration,
    rate_limit: &'static str,
    memo: Mutex<HashMap<usize, Option<Quote>>>,
    missing: Mutex<BTreeSet<usize>>,
    breaker: Breaker,
}

struct SharedQuotes(Arc<QuoteFeed>);

impl QuoteSource for SharedQuotes {
    fn quote(&self, bar: usize) -> Option<Quote> {
        self.0.quote(bar)
    }
}

impl QuoteFeed {
    /// The feed for dataset `ds` (bars stamped `parent`), or why it cannot price on bid/ask.
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        ctx: &Arc<Ctx>,
        ds: &hd::Dataset,
        parent: Vec<OffsetDateTime>,
        high: Vec<f64>,
        low: Vec<f64>,
    ) -> Result<QuoteFeed, String> {
        let connector = crate::histdata::connector_for(&ds.provider).map_err(|e| e.to_string())?;
        let kind = connector.quote_history(&ds.asset_type);
        let (Some(&first), Some(&last)) = (parent.first(), parent.last()) else {
            return Err("no bars".into());
        };
        let rows = hd::read_quotes(&ctx.state.pool, ds.id, first, last).await.unwrap_or_default();
        if kind == QuoteHistory::None && rows.is_empty() {
            return Err(format!("{} publishes no historical bid/ask", ds.provider));
        }
        let by_ts: HashMap<OffsetDateTime, Quote> = rows.into_iter().map(|q| (q.ts, q.quote)).collect();
        let stored = parent.iter().map(|t| by_ts.get(t).copied()).collect();
        let cap = connector.capability();
        Ok(QuoteFeed {
            ctx: ctx.clone(),
            key: IntrabarKey {
                provider: ds.provider.clone(),
                asset_type: ds.asset_type.clone(),
                ticker: ds.ticker.clone(),
                timeframe: ds.timeframe.clone(),
            },
            dataset: ds.id,
            kind,
            parent_secs: crate::histdata::timeframe_secs(&ds.timeframe).unwrap_or(60),
            parent,
            high,
            low,
            stored,
            page: cap.max_bars_per_req.max(1) as usize,
            min_interval: Duration::from_millis(cap.min_interval_ms),
            rate_limit: cap.rate_limit,
            memo: Mutex::new(HashMap::new()),
            missing: Mutex::new(BTreeSet::new()),
            breaker: Breaker::default(),
        })
    }

    pub fn boxed(self: &Arc<Self>) -> Box<dyn QuoteSource + Send + Sync> {
        Box::new(SharedQuotes(self.clone()))
    }

    fn missing(&self) -> Vec<usize> {
        self.missing.lock().map(|m| m.iter().copied().collect()).unwrap_or_default()
    }

    /// Requests that fetching `bars` would cost: two per bar for quotes read at the open and
    /// the close, one per page of consecutive bars for candles.
    fn requests_for(&self, bars: &[usize]) -> usize {
        match self.kind {
            QuoteHistory::Ticks => bars.len() * 2,
            QuoteHistory::Candles => {
                let mut pages = 0;
                let mut start: Option<usize> = None;
                for &b in bars {
                    if start.is_none_or(|s| b >= s + self.page) {
                        pages += 1;
                        start = Some(b);
                    }
                }
                pages
            }
            QuoteHistory::None => 0,
        }
    }

    fn end_of(&self, bar: usize) -> OffsetDateTime {
        self.parent
            .get(bar + 1)
            .copied()
            .unwrap_or(self.parent[bar] + time::Duration::seconds(self.parent_secs))
    }

    fn quote(&self, bar: usize) -> Option<Quote> {
        if let Some(q) = self.stored.get(bar).copied().flatten() {
            return Some(q);
        }
        if let Some(hit) = self.memo.lock().ok()?.get(&bar) {
            return *hit;
        }
        let p = &self.ctx.progress;
        let allowed = self.ctx.fetch_quotes
            && self.kind != QuoteHistory::None
            && !self.breaker.open()
            && !p.cancel.load(Ordering::Relaxed)
            && p.fetched.load(Ordering::Relaxed) < self.ctx.max_fetch;
        let got = if allowed {
            let handle = self.ctx.handle.clone();
            match tokio::task::block_in_place(|| handle.block_on(self.fetch(bar))) {
                Ok(q) => {
                    self.breaker.ok();
                    p.fetched.fetch_add(1, Ordering::Relaxed);
                    q
                }
                Err(e) => {
                    self.breaker.failed(&e);
                    p.failed.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!("quotes: {} {}: {e:#}", self.key.provider, self.key.ticker);
                    if let Ok(mut last) = p.last_error.lock() {
                        *last = Some(format!("{e:#}"));
                    }
                    None
                }
            }
        } else {
            None
        };
        if got.is_none() {
            if let Ok(mut m) = self.missing.lock() {
                m.insert(bar);
            }
        }
        if let Ok(mut m) = self.memo.lock() {
            m.insert(bar, got);
        }
        got
    }

    /// Fetch bar `bar`'s bid/ask, and with candles the page that starts there, stored on the
    /// dataset's own rows so a later run reads them for free.
    async fn fetch(&self, bar: usize) -> anyhow::Result<Option<Quote>> {
        let state = &self.ctx.state;
        let connector = crate::histdata::connector_for(&self.key.provider)?;
        let conn = conn_store::default_for(&state.pool, &self.key.provider).await?;
        let secrets = match &conn {
            Some(c) => conn_store::load_creds(&state.pool, &state.cipher, c.id).await?,
            None => Default::default(),
        };
        let k = &self.key;
        let count = |n: usize| async move {
            for _ in 0..n {
                crate::histdata_job::pace(&k.provider, self.min_interval).await;
                if let Some(c) = &conn {
                    let _ = otw_store::api_quota::bump(&state.pool, &crate::connectors_api::quota_scope(c.id)).await;
                }
                self.ctx.progress.requests.fetch_add(1, Ordering::Relaxed);
            }
        };
        match self.kind {
            QuoteHistory::Candles => {
                let last = (bar + self.page).min(self.parent.len()) - 1;
                let (from, to) = (self.parent[bar], self.end_of(last));
                count(1).await;
                let rows = connector
                    .fetch_quote_bars(&state.http, &secrets, &k.ticker, &k.asset_type, &k.timeframe, from, to)
                    .await?;
                if !rows.is_empty() {
                    hd::write_quotes(&state.pool, self.dataset, &rows).await?;
                }
                let index: HashMap<OffsetDateTime, usize> =
                    self.parent.iter().enumerate().map(|(i, t)| (*t, i)).collect();
                let mut found = None;
                if let Ok(mut memo) = self.memo.lock() {
                    for r in &rows {
                        if let Some(&i) = index.get(&r.ts) {
                            memo.insert(i, Some(r.quote));
                            if i == bar {
                                found = Some(r.quote);
                            }
                        }
                    }
                }
                Ok(found)
            }
            QuoteHistory::Ticks => {
                let (open, end) = (self.parent[bar], self.end_of(bar));
                count(2).await;
                let first = connector.quote_at(&state.http, &secrets, &k.ticker, &k.asset_type, open, false).await?;
                let close = connector.quote_at(&state.http, &secrets, &k.ticker, &k.asset_type, end, true).await?;
                let (Some(open_q), Some(close_q)) = (first, close) else { return Ok(None) };
                let q = tick_quote(open_q, close_q, self.high[bar], self.low[bar]);
                hd::write_quotes(&state.pool, self.dataset, &[hd::QuoteBar { ts: open, quote: q }]).await?;
                Ok(Some(q))
            }
            QuoteHistory::None => Ok(None),
        }
    }
}

/// Build the bid/ask feed of every asset, with the warnings for those that cannot have one.
/// A bar's bid/ask from the quotes at its open and its close, (bid, ask) each. Those two are
/// real. Inside the bar the spread is the one known at its open, around the bar's own extremes:
/// a fill inside the bar cannot be priced on a spread measured at its close.
fn tick_quote((bo, ao): (f64, f64), (bc, ac): (f64, f64), high: f64, low: f64) -> Quote {
    let half = (ao - bo).max(0.0) / 2.0;
    Quote {
        bid_open: bo,
        ask_open: ao,
        bid_close: bc,
        ask_close: ac,
        bid_high: high - half,
        ask_high: high + half,
        bid_low: low - half,
        ask_low: low + half,
    }
}

pub async fn quote_feeds(
    ctx: &Arc<Ctx>,
    ids: &[Uuid],
    bars: Vec<(Vec<OffsetDateTime>, Vec<f64>, Vec<f64>)>,
) -> (Vec<Option<Arc<QuoteFeed>>>, Vec<String>) {
    let mut out = Vec::with_capacity(ids.len());
    let mut warnings = Vec::new();
    for (&id, (parent, high, low)) in ids.iter().zip(bars) {
        let ds = match hd::get_dataset(&ctx.state.pool, id).await {
            Ok(Some(d)) => d,
            _ => {
                out.push(None);
                continue;
            }
        };
        match QuoteFeed::new(ctx, &ds, parent, high, low).await {
            Ok(q) => out.push(Some(Arc::new(q))),
            Err(why) => {
                warnings.push(format!("{}: {why}, fills use the spread", ds.ticker));
                out.push(None);
            }
        }
    }
    (out, warnings)
}

/// Build the lower-timeframe source of every asset that has a dataset, with the warnings for
/// those that cannot have one.
pub async fn sources(
    ctx: &Arc<Ctx>,
    ids: &[Uuid],
    parents: Vec<Vec<OffsetDateTime>>,
    ex: &Execution,
) -> (Vec<Option<Arc<Lower>>>, Vec<String>) {
    let mut out = Vec::with_capacity(ids.len());
    let mut warnings = Vec::new();
    for (&id, parent) in ids.iter().zip(parents) {
        let ds = match hd::get_dataset(&ctx.state.pool, id).await {
            Ok(Some(d)) => d,
            _ => {
                out.push(None);
                continue;
            }
        };
        match Lower::new(ctx, &ds, parent, ex).await {
            Ok(l) => out.push(Some(Arc::new(l))),
            Err(why) => {
                warnings.push(format!(
                    "{}: {why}, a bar where the stop and the target both print counts as a stop",
                    ds.ticker
                ));
                out.push(None);
            }
        }
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal that will not change stops the source at once; anything else after three in a
    /// row; a success in between starts the count again.
    /// #14 of the lookahead audit: the spread inside a tick-quoted bar is the one at its open,
    /// whatever the close's spread turned out to be.
    #[test]
    fn tick_quotes_price_the_bar_on_its_opening_spread() {
        let q = tick_quote((99.9, 100.1), (98.0, 102.0), 101.0, 99.0);
        assert!((q.ask_high - 101.1).abs() < 1e-9 && (q.bid_low - 98.9).abs() < 1e-9, "{q:?}");
        assert!((q.bid_close - 98.0).abs() < 1e-9 && (q.ask_close - 102.0).abs() < 1e-9);
    }

    #[test]
    fn downloads_stop_on_a_refusal_or_three_failures() {
        let b = Breaker::default();
        b.failed(&anyhow::anyhow!("VT 1d: No market data permissions for AMEX STK"));
        assert!(b.open());

        let b = Breaker::default();
        b.failed(&anyhow::anyhow!("no data in the window"));
        b.failed(&anyhow::anyhow!("no data in the window"));
        b.ok();
        b.failed(&anyhow::anyhow!("no data in the window"));
        b.failed(&anyhow::anyhow!("no data in the window"));
        assert!(!b.open());
        b.failed(&anyhow::anyhow!("no data in the window"));
        assert_eq!(b.reason().as_deref(), Some("no data in the window"));
    }
}
