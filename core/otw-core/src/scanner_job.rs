//! Scanner alerts, evaluated server-side on closed bars.
//!
//! The same shape as the chart alerts (`histviz_alerts`): the loop wakes a few seconds after
//! each minute boundary, and an item is read once a bar of its timeframe has closed since
//! the one it last saw. Then every enabled alert on that item ANDs its signals on the newest
//! closed bar.
//!
//! - **Closed bars only.** A forming candle is not evidence.
//! - **One read per item per bar.** An item scanned by three alerts over three hives is read
//!   once, at the widest window any of them needs ([`eval::read_plan`]).
//! - **A hit is (alert, item, bar).** The unique index dedupes across restarts and passes;
//!   the cooldown silences repeats on one item.
//! - **An alert that cannot run says so.** A refused key, a hive over the cap, a deleted
//!   signal or an item without bars is written to `last_error` and notified once per
//!   distinct error on the alert's channels, then once more when it runs again.
//! - **A board is watched too.** Items in a hive a board view shows are read on the same
//!   schedule without an alert, so the board has values; nothing is judged or notified.
//! - **Scripts queue.** A signal reading a script runs it per item on the bars read, under
//!   the permits the chart alerts share ([`crate::script::watched`]), off the async workers.
//! - **Watching is not downloading.** Bars come from the catalog when it is fresh, else from
//!   one bounded request through the connector granted to the scanner; nothing is stored
//!   but the board's cache.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::backtest as bt_store;
use otw_store::connectors as conn_store;
use otw_store::crypto::SecretCipher;
use otw_store::histdata::{self as hist_store, Bar};
use otw_store::reminders::FiredNotification;
use otw_store::scanner::{self as store, Alert, Item, ItemCache};

use crate::backtest::custom::CustomIndicatorDef;
use crate::backtest::script::ScriptDef;
use crate::histviz_alerts::{closed_bars, short};
use crate::notif_send;
use crate::scanner::eval::{self, Compiled, HitLine, Library, Message, Verdict};
use crate::scanner::Ohlcv;

/// Module id for grants, notifications and the data broker.
pub const MODULE: &str = "scanner";
/// Seconds past each minute boundary the loop wakes: a provider needs a moment to publish.
const SETTLE_SECS: i64 = 4;
const WARMUP: Duration = Duration::from_secs(25);
/// An item is never read twice within this, whatever the wake-ups.
const MIN_GAP: i64 = 55;
/// An item without a bar yet, failing, or waiting on a late bar is retried after its bar
/// length, bounded: a weekend of daily bars costs one read a quarter hour, not one a minute.
const MIN_RETRY: i64 = 60;
const MAX_RETRY: i64 = 900;
/// Hits older than this are dropped by the nightly prune.
const HIT_DAYS: i64 = 90;
/// Run now reads this many items at once, and gives up on the rest after the deadline.
const PREVIEW_CONCURRENCY: usize = 6;
const PREVIEW_DEADLINE: Duration = Duration::from_secs(45);

#[derive(Clone)]
pub struct ScanCx {
    pub cipher: SecretCipher,
    pub http: Client,
}

pub fn spawn(pool: PgPool, cx: ScanCx) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        loop {
            tokio::time::sleep(until_next_wake(OffsetDateTime::now_utc())).await;
            if let Err(e) = pass(&pool, &cx).await {
                tracing::error!("scanner: pass failed: {e:#}");
            }
        }
    });
}

fn until_next_wake(now: OffsetDateTime) -> Duration {
    let ms = (now.unix_timestamp_nanos() / 1_000_000) as i64;
    let this = ms.div_euclid(60_000) * 60_000 + SETTLE_SECS * 1000;
    let at = if this > ms { this } else { this + 60_000 };
    Duration::from_millis((at - ms) as u64)
}

/// One sweep over every enabled alert and every item a board shows.
async fn pass(pool: &PgPool, cx: &ScanCx) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    if now.hour() == 3 && now.minute() == 0 {
        let gone = store::prune_hits(pool, HIT_DAYS).await?;
        if gone > 0 {
            tracing::info!("scanner: pruned {gone} hits older than {HIT_DAYS} days");
        }
    }
    let alerts = store::enabled_alerts(pool).await?;
    let watched = store::board_items(pool).await?;
    if alerts.is_empty() && watched.is_empty() {
        return Ok(());
    }
    let cap = store::hive_cap(pool).await?;
    let signals = compile_all(pool, alerts.iter().flat_map(|a| a.signal_ids.iter().copied())).await?;
    let mut targets = HashMap::new();
    for a in &alerts {
        targets.insert(a.id, target_items(pool, a, cap).await?);
    }
    let mut io = DbIo { pool, cx };
    let report = run(&mut io, PassInput { now, alerts: &alerts, targets, signals: &signals, watched }).await?;
    if report.reads > 0 || report.hits > 0 {
        tracing::debug!("scanner: {} reads, {} hits", report.reads, report.hits);
    }
    Ok(())
}

/// An alert's items, or why it has none to scan.
pub(crate) async fn target_items(pool: &PgPool, a: &Alert, cap: i64) -> Result<Result<Vec<Item>, String>> {
    Ok(match a.target_kind.as_str() {
        "item" => match store::get_item(pool, a.target_id).await? {
            Some(i) => Ok(vec![i]),
            None => Err("The item this alert scans was deleted.".to_string()),
        },
        _ => match store::get_hive(pool, a.target_id).await? {
            None => Err("The hive this alert scans was deleted.".to_string()),
            Some(h) => {
                if let Some(why) = store::cap_refusal(h.item_ids.len() as i64, cap) {
                    Err(format!("{}: {why}", h.name))
                } else if h.item_ids.is_empty() {
                    Err(format!("The hive {} holds no items: add some in the Items tab.", h.name))
                } else {
                    Ok(store::items_by_ids(pool, &h.item_ids).await?)
                }
            }
        },
    })
}

/// Load the signals by id and resolve them against the library. A signal that cannot be
/// judged is kept as its reason, so the alerts that use it can say so.
pub(crate) async fn compile_all(
    pool: &PgPool,
    ids: impl IntoIterator<Item = Uuid>,
) -> Result<HashMap<Uuid, Result<Compiled, String>>> {
    let mut wanted: Vec<Uuid> = ids.into_iter().collect();
    wanted.sort();
    wanted.dedup();
    let rows = store::signals_by_ids(pool, &wanted).await?;
    let specs: Vec<(Uuid, String, Result<eval::Spec, String>)> =
        rows.iter().map(|r| (r.id, r.name.clone(), eval::parse(&r.builder, &r.config))).collect();
    let lib = library_for(pool, specs.iter().filter_map(|(_, _, s)| s.as_ref().ok())).await?;
    let mut out = HashMap::new();
    for (id, name, spec) in specs {
        out.insert(id, spec.and_then(|s| Compiled::new(id, &name, s, &lib)));
    }
    Ok(out)
}

/// The library rows and sessions these specs read.
pub(crate) async fn library_for<'a>(pool: &PgPool, specs: impl Iterator<Item = &'a eval::Spec>) -> Result<Library> {
    let mut lib = Library::default();
    let mut need_sessions = false;
    for s in specs {
        need_sessions |= !s.session_ids().is_empty();
        for id in s.custom_ids() {
            if lib.dag.contains_key(&id) || lib.scripts.contains_key(&id) {
                continue;
            }
            let Ok(uuid) = Uuid::parse_str(&id) else { continue };
            let Some(row) = bt_store::get_indicator(pool, uuid).await? else { continue };
            if row.kind == "script" {
                // A scanner signal runs a script with its default inputs.
                let def = ScriptDef { name: row.name, source: row.source.unwrap_or_default(), inputs: BTreeMap::new() };
                lib.scripts.insert(id, def);
                continue;
            }
            let Some(def) = row.definition else { continue };
            if let Ok(def) = serde_json::from_value::<CustomIndicatorDef>(def) {
                if def.validate().is_none() {
                    lib.dag.insert(id, def);
                }
            }
        }
    }
    if need_sessions {
        lib.sessions = otw_store::market_sessions::list(pool).await?;
    }
    Ok(lib)
}

// ── The pass, over an IO the tests can script ────────────────────────────────

/// Everything the pass touches outside itself.
pub(crate) trait ScanIo {
    async fn read(&mut self, item: &Item, want: usize, now: OffsetDateTime) -> Result<Vec<Bar>, String>;
    async fn save_cache(&mut self, item: Uuid, c: &ItemCache) -> Result<()>;
    async fn item_checked(&mut self, item: Uuid) -> Result<()>;
    async fn item_error(&mut self, item: Uuid, msg: &str) -> Result<()>;
    async fn merge_state(&mut self, item: Uuid, state: &Value) -> Result<()>;
    async fn last_hit_bar(&mut self, alert: Uuid, item: Uuid) -> Result<Option<OffsetDateTime>>;
    async fn record_hit(&mut self, alert: Uuid, item: Uuid, bar_ts: OffsetDateTime, values: &Value) -> Result<Option<Uuid>>;
    async fn notify(&mut self, alert: &Alert, m: &Message) -> Result<()>;
    async fn hits_notified(&mut self, ids: &[Uuid]) -> Result<()>;
    async fn alert_ran(&mut self, alert: Uuid, error: &str) -> Result<()>;
    /// The instrument facts a script reads for this item.
    async fn syminfo(&mut self, item: &Item) -> Result<otw_script::input::SymInfo>;
}

pub(crate) struct PassInput<'a> {
    pub now: OffsetDateTime,
    pub alerts: &'a [Alert],
    pub targets: HashMap<Uuid, Result<Vec<Item>, String>>,
    pub signals: &'a HashMap<Uuid, Result<Compiled, String>>,
    /// Items a board shows: read for the cache whether or not an alert scans them.
    pub watched: Vec<Item>,
}

/// Bars a board item is read at: its sparkline plus the margin.
const BOARD_BARS: usize = eval::SPARK_BARS + eval::EXTRA_BARS;

#[derive(Debug, Default)]
pub(crate) struct PassReport {
    pub reads: usize,
    pub hits: usize,
}

/// The signals of an alert, or why one of them cannot be judged.
fn alert_signals<'a>(
    a: &Alert,
    signals: &'a HashMap<Uuid, Result<Compiled, String>>,
) -> Result<Vec<&'a Compiled>, String> {
    a.signal_ids
        .iter()
        .map(|id| match signals.get(id) {
            Some(Ok(c)) => Ok(c),
            Some(Err(why)) => Err(why.clone()),
            None => Err("A signal of this alert was deleted: pick another one.".to_string()),
        })
        .collect()
}

pub(crate) async fn run<I: ScanIo>(io: &mut I, input: PassInput<'_>) -> Result<PassReport> {
    let now = input.now;
    let mut report = PassReport::default();

    // What each alert scans, and with what; an alert that cannot start keeps its reason.
    let mut ready: Vec<(&Alert, Vec<&Compiled>, &Vec<Item>)> = Vec::new();
    let mut fatal: Vec<(&Alert, String)> = Vec::new();
    for a in input.alerts {
        let target = input.targets.get(&a.id);
        match (target, alert_signals(a, input.signals)) {
            (Some(Ok(items)), Ok(sigs)) => ready.push((a, sigs, items)),
            (Some(Err(why)), _) => fatal.push((a, why.clone())),
            (_, Err(why)) => fatal.push((a, why)),
            (None, _) => fatal.push((a, "This alert has no target.".to_string())),
        }
    }

    // One read per item, at the widest window its alerts or the board need.
    let mut items: BTreeMap<Uuid, &Item> = BTreeMap::new();
    let mut wants = Vec::new();
    for it in &input.watched {
        items.insert(it.id, it);
        wants.push((it.id, BOARD_BARS));
    }
    for (_, sigs, list) in &ready {
        for it in list.iter() {
            items.insert(it.id, it);
            if let Ok(tf) = crate::histdata::timeframe_secs(&it.timeframe) {
                wants.push((it.id, eval::bars_wanted(sigs, tf)));
            }
        }
    }
    let plan = eval::read_plan(wants);
    let mut fresh: HashMap<Uuid, Ohlcv> = HashMap::new();
    let mut failed: HashMap<Uuid, String> = HashMap::new();
    for (id, it) in &items {
        let Ok(tf) = crate::histdata::timeframe_secs(&it.timeframe) else {
            failed.insert(*id, format!("unknown timeframe {}", it.timeframe));
            continue;
        };
        let Some(&want) = plan.get(id) else { continue };
        if !item_due(it, tf, now) {
            continue;
        }
        report.reads += 1;
        match io.read(it, want, now).await {
            Err(e) => {
                io.item_error(*id, &e).await?;
                failed.insert(*id, e);
            }
            Ok(bars) => {
                let mut closed = closed_bars(&bars, tf, now);
                if closed.is_empty() {
                    let why = format!(
                        "no closed {} bar came back for {}: check the symbol, or the connector's history",
                        it.timeframe, it.ticker
                    );
                    io.item_error(*id, &why).await?;
                    failed.insert(*id, why);
                    continue;
                }
                if closed.len() > want {
                    closed.drain(..closed.len() - want);
                }
                let newest = closed.last().map(|b| b.ts);
                if newest > it.last_bar_ts {
                    let s = Ohlcv::of(&closed);
                    io.save_cache(*id, &eval::cache_of(&s, tf)).await?;
                    fresh.insert(*id, s);
                } else {
                    io.item_checked(*id).await?;
                }
            }
        }
    }

    // Judge each alert on the items that brought a new closed bar.
    let mut states: HashMap<Uuid, Map<String, Value>> = HashMap::new();
    for (a, sigs, list) in &ready {
        let mut errs: Vec<String> = Vec::new();
        let mut lines: Vec<HitLine> = Vec::new();
        let mut ids: Vec<Uuid> = Vec::new();
        let mut touched = false;
        for it in list.iter() {
            if let Some(e) = failed.get(&it.id) {
                touched = true;
                let line = sentence(&format!("{}: {e}", it.ticker));
                if !errs.contains(&line) {
                    errs.push(line);
                }
                continue;
            }
            let Some(s) = fresh.get(&it.id) else { continue };
            touched = true;
            let Ok(tf) = crate::histdata::timeframe_secs(&it.timeframe) else { continue };
            let Some(v) = judged(sigs, s, it, tf, io.syminfo(it)).await? else { continue };
            let st = states.entry(it.id).or_default();
            for (sid, held) in &v.states {
                st.insert(sid.to_string(), json!(held));
            }
            if !v.held {
                continue;
            }
            let last = io.last_hit_bar(a.id, it.id).await?;
            if !eval::cooldown_allows(last, v.bar_ts, a.cooldown_secs) {
                continue;
            }
            if let Some(hid) = io.record_hit(a.id, it.id, v.bar_ts, &v.values).await? {
                ids.push(hid);
                lines.push(HitLine {
                    provider: it.provider.clone(),
                    asset_type: it.asset_type.clone(),
                    ticker: it.ticker.clone(),
                    timeframe: it.timeframe.clone(),
                    connector_id: it.connector_id,
                    bar_ts: v.bar_ts,
                    values: v.values.clone(),
                });
            }
        }
        if !touched {
            continue;
        }
        report.hits += ids.len();
        for m in eval::compose(a.id, &a.name, &a.notify, &lines) {
            io.notify(a, &m).await?;
        }
        if !ids.is_empty() {
            io.hits_notified(&ids).await?;
        }
        let error = joined(&errs);
        settle(io, a, &error).await?;
    }
    for (a, why) in fatal {
        settle(io, a, &why).await?;
    }
    for (id, st) in states {
        io.merge_state(id, &Value::Object(st)).await?;
    }
    Ok(report)
}

/// AND the signals on an item's newest closed bar. Signals running a script are judged under
/// a watcher permit, on a history built for the item; `syminfo` is awaited only then.
pub(crate) async fn judged(
    sigs: &[&Compiled],
    s: &Ohlcv,
    it: &Item,
    tf: i64,
    syminfo: impl std::future::Future<Output = Result<otw_script::input::SymInfo>>,
) -> Result<Option<Verdict>> {
    if !eval::reads_script(sigs) {
        return Ok(eval::verdict(sigs, s, tf, None));
    }
    let h = eval::script_history(syminfo.await?, &it.timeframe);
    Ok(crate::script::watched(|| eval::verdict(sigs, s, tf, Some(&h))).await)
}

/// The instrument facts a script reads for an item: the resolved or stored metadata.
pub(crate) async fn syminfo(pool: &PgPool, it: &Item) -> Result<otw_script::input::SymInfo> {
    let meta = crate::instrument_meta_api::resolved_or_stored(pool, &it.provider, &it.asset_type, &it.ticker).await?;
    Ok(crate::script::syminfo_for(&it.provider, &it.asset_type, &it.ticker, meta.as_ref()))
}

/// Record how the run ended, and tell the user when that is news.
async fn settle<I: ScanIo>(io: &mut I, a: &Alert, error: &str) -> Result<()> {
    let notice = eval::transition(&a.last_error, error);
    if let Some(m) = eval::notice_message(a.id, &a.name, &notice) {
        io.notify(a, &m).await?;
    }
    io.alert_ran(a.id, error).await
}

/// A line that ends like a sentence, so several read as several.
fn sentence(s: &str) -> String {
    let s = s.trim();
    if s.ends_with(['.', '!', '?']) { s.to_string() } else { format!("{s}.") }
}

/// The item errors of one run as one line the alert list can show.
fn joined(errs: &[String]) -> String {
    if errs.is_empty() {
        return String::new();
    }
    let mut s = errs.iter().take(3).cloned().collect::<Vec<_>>().join(" ");
    if errs.len() > 3 {
        s.push_str(&format!(" And {} more items failed.", errs.len() - 3));
    }
    s
}

/// Whether this item has a bar worth reading now. Once it has a bar, it is due when the
/// next one has closed, and then every retry interval until the provider has it. Before
/// its first bar, or after a failure, it waits its bar length, bounded.
pub(crate) fn item_due(it: &Item, tf: i64, now: OffsetDateTime) -> bool {
    let Some(checked) = it.checked_at else { return true };
    let since = (now - checked).whole_seconds();
    if since < MIN_GAP {
        return false;
    }
    let retry = tf.clamp(MIN_RETRY, MAX_RETRY);
    let waited = since + (60 - MIN_GAP) >= retry;
    match it.last_bar_ts {
        Some(judged) if it.last_error.is_empty() => {
            let expected = judged + time::Duration::seconds(2 * tf);
            now >= expected && (checked < expected || waited)
        }
        _ => waited,
    }
}

// ── The database and the providers ───────────────────────────────────────────

struct DbIo<'a> {
    pool: &'a PgPool,
    cx: &'a ScanCx,
}

impl ScanIo for DbIo<'_> {
    async fn read(&mut self, item: &Item, want: usize, now: OffsetDateTime) -> Result<Vec<Bar>, String> {
        load_bars(self.pool, self.cx, item, want, now).await.map_err(|e| short(&format!("{e:#}")))
    }
    async fn save_cache(&mut self, item: Uuid, c: &ItemCache) -> Result<()> {
        store::save_item_cache(self.pool, item, c).await
    }
    async fn item_checked(&mut self, item: Uuid) -> Result<()> {
        store::mark_item_checked(self.pool, item).await
    }
    async fn item_error(&mut self, item: Uuid, msg: &str) -> Result<()> {
        store::mark_item_error(self.pool, item, msg).await
    }
    async fn merge_state(&mut self, item: Uuid, state: &Value) -> Result<()> {
        store::merge_signal_state(self.pool, item, state).await
    }
    async fn last_hit_bar(&mut self, alert: Uuid, item: Uuid) -> Result<Option<OffsetDateTime>> {
        store::last_hit_bar(self.pool, alert, item).await
    }
    async fn record_hit(&mut self, alert: Uuid, item: Uuid, bar_ts: OffsetDateTime, values: &Value) -> Result<Option<Uuid>> {
        store::insert_hit(self.pool, alert, item, bar_ts, values).await
    }
    async fn notify(&mut self, alert: &Alert, m: &Message) -> Result<()> {
        notify(self.pool, self.cx, alert, m).await
    }
    async fn hits_notified(&mut self, ids: &[Uuid]) -> Result<()> {
        store::mark_hits_notified(self.pool, ids).await
    }
    async fn alert_ran(&mut self, alert: Uuid, error: &str) -> Result<()> {
        store::mark_alert_run(self.pool, alert, error).await
    }
    async fn syminfo(&mut self, item: &Item) -> Result<otw_script::input::SymInfo> {
        syminfo(self.pool, item).await
    }
}

/// The in-app notification, linked to the alert, pushed to the alert's channels.
async fn notify(pool: &PgPool, cx: &ScanCx, a: &Alert, m: &Message) -> Result<()> {
    sqlx::query(
        "INSERT INTO notifications (id, name, kind, linked_id, details, url, link_label) \
         VALUES ($1,$2,'scanner',$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(&m.title)
    .bind(a.id)
    .bind(&m.details)
    .bind(&m.url)
    .bind(&m.link_label)
    .execute(pool)
    .await?;
    let channels: Vec<Uuid> = a
        .channels
        .as_array()
        .map(|v| v.iter().filter_map(|x| x.as_str()).filter_map(|s| Uuid::parse_str(s).ok()).collect())
        .unwrap_or_default();
    let notif = FiredNotification { name: m.title.clone(), details: m.details.clone(), url: m.url.clone() };
    // Empty = every channel the scanner is granted, the broker's convention.
    notif_send::dispatch(pool, &cx.cipher, &cx.http, &notif, MODULE, if channels.is_empty() { None } else { Some(&channels) })
        .await;
    Ok(())
}

/// The connector an item reads through: its own when the scanner may use it, else the
/// oldest one granted to the scanner for that provider.
pub(crate) async fn resolve_connector(pool: &PgPool, it: &Item) -> Result<Uuid> {
    if let Some(id) = it.connector_id {
        if let Some(c) = conn_store::get(pool, id).await? {
            if c.allows(MODULE) {
                return Ok(c.id);
            }
            return Err(anyhow!(
                "the connector {} reads through ({}) is not granted to the Scanner: grant it in the data broker",
                it.ticker,
                c.name
            ));
        }
    }
    conn_store::default_for_module(pool, &it.provider, MODULE)
        .await?
        .map(|c| c.id)
        .ok_or_else(|| {
            anyhow!(
                "no {} connector is granted to the Scanner, so {} cannot be read: grant one in the data broker",
                it.provider,
                it.ticker
            )
        })
}

/// Up to `want` recent bars: the catalog when it reaches the present, else one bounded
/// request through the item's connector. Nothing is stored.
pub(crate) async fn load_bars(pool: &PgPool, cx: &ScanCx, it: &Item, want: usize, now: OffsetDateTime) -> Result<Vec<Bar>> {
    let tf = crate::histdata::timeframe_secs(&it.timeframe).map_err(|e| anyhow!("{e}"))?;
    let want = want as i64;
    let dataset = hist_store::find_dataset(pool, &it.provider, &it.asset_type, &it.ticker, &it.timeframe).await?;
    let mut rows = match &dataset {
        Some(d) => hist_store::read_bars(pool, d.id, None, Some(now), want + 2).await?,
        None => Vec::new(),
    };
    let fresh = rows.last().is_some_and(|b| (now - b.ts).whole_seconds() < tf * 2);
    if !fresh {
        let connector = resolve_connector(pool, it).await?;
        let secrets = conn_store::load_creds(pool, &cx.cipher, connector).await?;
        let rest = crate::histdata::connector_for(&it.provider)?;
        // Markets close: twice the window in clock time still lands `want` bars on a stock.
        let span = tf.saturating_mul(want * 2 + 2).min(tf.saturating_mul(eval::MAX_BARS as i64 * 2));
        let start = rows
            .last()
            .map(|b| b.ts)
            .unwrap_or(now - time::Duration::seconds(span))
            .max(now - time::Duration::seconds(span));
        let chunk = rest
            .fetch_chunk(&cx.http, &secrets, &it.ticker, &it.asset_type, &it.timeframe, start, now)
            .await
            .with_context(|| format!("reading {} {}", it.ticker, it.timeframe))?;
        rows.extend(chunk.bars);
        rows.sort_by_key(|b| b.ts);
        rows.dedup_by_key(|b| b.ts);
    }
    Ok(rows)
}

// ── Run now ──────────────────────────────────────────────────────────────────

/// One item of a run-now preview.
#[derive(Debug, Serialize)]
pub struct PreviewRow {
    pub item_id: Uuid,
    pub ticker: String,
    pub timeframe: String,
    pub held: bool,
    /// Each signal's state on the newest closed bar, by id.
    pub states: BTreeMap<String, bool>,
    pub values: Value,
    #[serde(with = "time::serde::rfc3339::option")]
    pub bar_ts: Option<OffsetDateTime>,
    pub error: String,
}

/// Judge an alert's signals on each of its items now, without recording a hit or
/// notifying. Bounded: a few reads at once, and what has not answered by the deadline is
/// reported as such.
pub(crate) async fn preview(pool: &PgPool, cx: &ScanCx, items: Vec<Item>, sigs: &[&Compiled]) -> Vec<PreviewRow> {
    let now = OffsetDateTime::now_utc();
    let order: Vec<(Uuid, String, String)> =
        items.iter().map(|i| (i.id, i.ticker.clone(), i.timeframe.clone())).collect();
    let mut rows: HashMap<Uuid, PreviewRow> = HashMap::new();
    let jobs = items.into_iter().map(|it| async move {
        let row = preview_one(pool, cx, &it, sigs, now).await;
        (it.id, row)
    });
    let mut stream = futures::stream::iter(jobs).buffer_unordered(PREVIEW_CONCURRENCY);
    let deadline = tokio::time::Instant::now() + PREVIEW_DEADLINE;
    while let Ok(Some((id, row))) = tokio::time::timeout_at(deadline, stream.next()).await {
        rows.insert(id, row);
    }
    drop(stream);
    order
        .into_iter()
        .map(|(id, ticker, timeframe)| {
            rows.remove(&id).unwrap_or_else(|| PreviewRow {
                item_id: id,
                ticker,
                timeframe,
                held: false,
                states: BTreeMap::new(),
                values: json!({}),
                bar_ts: None,
                error: "not read before the time limit: run it again, or scan a smaller hive".into(),
            })
        })
        .collect()
}

async fn preview_one(pool: &PgPool, cx: &ScanCx, it: &Item, sigs: &[&Compiled], now: OffsetDateTime) -> PreviewRow {
    let mut row = PreviewRow {
        item_id: it.id,
        ticker: it.ticker.clone(),
        timeframe: it.timeframe.clone(),
        held: false,
        states: BTreeMap::new(),
        values: json!({}),
        bar_ts: None,
        error: String::new(),
    };
    let tf = match crate::histdata::timeframe_secs(&it.timeframe) {
        Ok(tf) => tf,
        Err(e) => {
            row.error = e.to_string();
            return row;
        }
    };
    let want = eval::bars_wanted(sigs, tf);
    let bars = match load_bars(pool, cx, it, want, now).await {
        Ok(b) => b,
        Err(e) => {
            row.error = short(&format!("{e:#}"));
            let _ = store::mark_item_error(pool, it.id, &row.error).await;
            return row;
        }
    };
    let mut closed = closed_bars(&bars, tf, now);
    if closed.len() > want {
        closed.drain(..closed.len() - want);
    }
    let s = Ohlcv::of(&closed);
    let v = match judged(sigs, &s, it, tf, syminfo(pool, it)).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            row.error = format!("no closed {} bar came back for {}", it.timeframe, it.ticker);
            return row;
        }
        Err(e) => {
            row.error = short(&format!("{e:#}"));
            return row;
        }
    };
    let _ = store::save_item_cache(pool, it.id, &eval::cache_of(&s, tf)).await;
    let mut st = Map::new();
    for (id, held) in &v.states {
        row.states.insert(id.to_string(), *held);
        st.insert(id.to_string(), json!(held));
    }
    let _ = store::merge_signal_state(pool, it.id, &Value::Object(st)).await;
    row.held = v.held;
    row.values = v.values;
    row.bar_ts = Some(v.bar_ts);
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::eval::{parse, Library};
    use std::collections::HashSet;
    use time::macros::datetime;

    fn item(n: u128, ticker: &str) -> Item {
        Item {
            id: Uuid::from_u128(n),
            provider: "binance".into(),
            asset_type: "crypto".into(),
            ticker: ticker.into(),
            timeframe: "1h".into(),
            connector_id: None,
            label: String::new(),
            color: String::new(),
            last_close: None,
            last_open: None,
            last_volume: None,
            avg_volume: None,
            prev_close: None,
            day_open: None,
            last_bar_ts: None,
            spark: json!([]),
            signal_state: json!({}),
            checked_at: None,
            last_error: String::new(),
            updated_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            hive_ids: vec![],
        }
    }

    fn alert(n: u128, signals: Vec<Uuid>, notify: &str) -> Alert {
        Alert {
            id: Uuid::from_u128(1000 + n),
            name: format!("A{n}"),
            enabled: true,
            target_kind: "hive".into(),
            target_id: Uuid::from_u128(500),
            signal_ids: signals,
            notify: notify.into(),
            channels: json!([]),
            cooldown_secs: 0,
            last_run_at: None,
            last_error: String::new(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    fn bar(ts: OffsetDateTime, o: f64, h: f64, l: f64, c: f64) -> Bar {
        Bar { ts, open: o, high: h, low: l, close: c, volume: 100.0, adj_open: None, adj_high: None, adj_low: None, adj_close: None }
    }

    /// An in-memory database and provider: scripted bars per ticker, a refusal per ticker,
    /// and a unique set standing for the hits index.
    #[derive(Default)]
    struct Fake {
        bars: HashMap<String, Vec<Bar>>,
        refuse: HashMap<String, String>,
        reads: Vec<String>,
        hits: HashSet<(Uuid, Uuid, i64)>,
        notes: Vec<Message>,
        errors: HashMap<Uuid, String>,
        caches: HashMap<Uuid, ItemCache>,
    }

    impl ScanIo for Fake {
        async fn read(&mut self, item: &Item, _want: usize, _now: OffsetDateTime) -> Result<Vec<Bar>, String> {
            self.reads.push(item.ticker.clone());
            if let Some(e) = self.refuse.get(&item.ticker) {
                return Err(e.clone());
            }
            Ok(self.bars.get(&item.ticker).cloned().unwrap_or_default())
        }
        async fn save_cache(&mut self, item: Uuid, c: &ItemCache) -> Result<()> {
            self.caches.insert(item, c.clone());
            Ok(())
        }
        async fn item_checked(&mut self, _item: Uuid) -> Result<()> {
            Ok(())
        }
        async fn item_error(&mut self, _item: Uuid, _msg: &str) -> Result<()> {
            Ok(())
        }
        async fn merge_state(&mut self, _item: Uuid, _state: &Value) -> Result<()> {
            Ok(())
        }
        async fn last_hit_bar(&mut self, _alert: Uuid, _item: Uuid) -> Result<Option<OffsetDateTime>> {
            Ok(None)
        }
        async fn record_hit(&mut self, alert: Uuid, item: Uuid, bar_ts: OffsetDateTime, _values: &Value) -> Result<Option<Uuid>> {
            Ok(self.hits.insert((alert, item, bar_ts.unix_timestamp())).then(Uuid::new_v4))
        }
        async fn notify(&mut self, _alert: &Alert, m: &Message) -> Result<()> {
            self.notes.push(m.clone());
            Ok(())
        }
        async fn hits_notified(&mut self, _ids: &[Uuid]) -> Result<()> {
            Ok(())
        }
        async fn alert_ran(&mut self, alert: Uuid, error: &str) -> Result<()> {
            self.errors.insert(alert, error.to_string());
            Ok(())
        }
        async fn syminfo(&mut self, item: &Item) -> Result<otw_script::input::SymInfo> {
            Ok(crate::script::syminfo_for(&item.provider, &item.asset_type, &item.ticker, None))
        }
    }

    fn compiled(id: u128, builder: &str, config: Value) -> (Uuid, Result<Compiled, String>) {
        let uid = Uuid::from_u128(id);
        let spec = parse(builder, &config).unwrap();
        (uid, Compiled::new(uid, builder, spec, &Library::default()))
    }

    /// Five items in one hive, hourly bars to 10:00. Items 1 and 3 end on a bullish
    /// engulfing bar that also closed higher; the others drift.
    fn scripted() -> (Fake, Vec<Item>) {
        let mut f = Fake::default();
        let mut items = Vec::new();
        for (n, t) in ["AAA", "BBB", "CCC", "DDD", "EEE"].iter().enumerate() {
            let it = item(n as u128 + 1, t);
            let t0 = datetime!(2026-09-07 06:00 UTC);
            let mut bs: Vec<Bar> = (0..4)
                .map(|k| bar(t0 + time::Duration::hours(k), 100.0, 100.6, 99.4, 100.2))
                .collect();
            if n % 2 == 0 && n < 4 {
                bs.push(bar(t0 + time::Duration::hours(4), 100.5, 100.6, 99.6, 99.8));
                bs.push(bar(t0 + time::Duration::hours(5), 99.7, 101.2, 99.6, 101.0));
            } else {
                bs.push(bar(t0 + time::Duration::hours(4), 100.2, 100.5, 100.0, 100.3));
                bs.push(bar(t0 + time::Duration::hours(5), 100.3, 100.6, 100.1, 100.4));
            }
            // A forming bar: never judged.
            bs.push(bar(t0 + time::Duration::hours(6), 101.0, 150.0, 50.0, 140.0));
            f.bars.insert(t.to_string(), bs);
            items.push(it);
        }
        (f, items)
    }

    fn input<'a>(
        now: OffsetDateTime,
        alerts: &'a [Alert],
        items: &[Item],
        signals: &'a HashMap<Uuid, Result<Compiled, String>>,
    ) -> PassInput<'a> {
        PassInput {
            now,
            alerts,
            targets: alerts.iter().map(|a| (a.id, Ok(items.to_vec()))).collect(),
            signals,
            watched: vec![],
        }
    }

    #[tokio::test]
    async fn scanner_job_signal_pattern_and_time_hit_once_and_dedupe_on_a_second_pass() {
        let (mut f, items) = scripted();
        let signals: HashMap<Uuid, Result<Compiled, String>> = [
            compiled(1, "signal", json!({"conditions": [{"left": {"kind": "price", "field": "close"}, "op": "rising"}]})),
            compiled(2, "pattern", json!({"kind": "engulfing", "direction": "up"})),
            compiled(3, "time", json!({"from": "08:00", "to": "20:00", "timezone": "UTC"})),
        ]
        .into_iter()
        .collect();
        let alerts = vec![alert(1, vec![Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3)], "digest")];
        let now = datetime!(2026-09-07 12:30:04 UTC);
        let r = run(&mut f, input(now, &alerts, &items, &signals)).await.unwrap();
        assert_eq!(r.reads, 5);
        assert_eq!(r.hits, 2, "{:?}", f.notes);
        assert_eq!(f.notes.len(), 1, "a digest is one notification");
        assert!(f.notes[0].title.ends_with("2 hits"));
        assert!(f.notes[0].details.contains("AAA") && f.notes[0].details.contains("CCC"));
        assert_eq!(f.errors[&alerts[0].id], "");
        // The forming 12:00 bar was not judged: the cache stops at 11:00.
        assert_eq!(f.caches[&items[0].id].last_bar_ts, Some(datetime!(2026-09-07 11:00 UTC)));

        // A second pass over the same bars (a restart) records nothing new.
        let r = run(&mut f, input(now, &alerts, &items, &signals)).await.unwrap();
        assert_eq!(r.hits, 0);
        assert_eq!(f.notes.len(), 1);
    }

    #[tokio::test]
    async fn scanner_job_per_hit_notifies_each_hit() {
        let (mut f, items) = scripted();
        let signals: HashMap<Uuid, Result<Compiled, String>> =
            [compiled(2, "pattern", json!({"kind": "engulfing", "direction": "up"}))].into_iter().collect();
        let alerts = vec![alert(1, vec![Uuid::from_u128(2)], "per_hit")];
        run(&mut f, input(datetime!(2026-09-07 12:30:04 UTC), &alerts, &items, &signals)).await.unwrap();
        assert_eq!(f.notes.len(), 2);
        assert!(f.notes.iter().all(|n| n.url.starts_with("/histviz?")));
    }

    #[tokio::test]
    async fn scanner_job_items_shared_by_alerts_are_read_once() {
        let (mut f, items) = scripted();
        let signals: HashMap<Uuid, Result<Compiled, String>> = [
            compiled(1, "signal", json!({"conditions": [{"left": {"kind": "price", "field": "close"}, "op": "rising"}]})),
            compiled(2, "pattern", json!({"kind": "doji"})),
        ]
        .into_iter()
        .collect();
        // Two alerts reuse signal 1 over the same hive.
        let alerts = vec![
            alert(1, vec![Uuid::from_u128(1)], "digest"),
            alert(2, vec![Uuid::from_u128(1), Uuid::from_u128(2)], "digest"),
        ];
        let r = run(&mut f, input(datetime!(2026-09-07 12:30:04 UTC), &alerts, &items, &signals)).await.unwrap();
        assert_eq!(r.reads, 5);
        let mut seen = f.reads.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), f.reads.len(), "each item read once: {:?}", f.reads);
    }

    #[tokio::test]
    async fn scanner_job_refused_connector_reported_once_then_recovery() {
        let (mut f, items) = scripted();
        let items = items[..1].to_vec();
        f.refuse.insert("AAA".into(), "the key was refused".into());
        let signals: HashMap<Uuid, Result<Compiled, String>> =
            [compiled(2, "pattern", json!({"kind": "doji"}))].into_iter().collect();
        let mut a = alert(1, vec![Uuid::from_u128(2)], "digest");
        let mut it = items[0].clone();

        // First failure: notified.
        run(&mut f, input(datetime!(2026-09-07 12:30:04 UTC), std::slice::from_ref(&a), &[it.clone()], &signals))
            .await
            .unwrap();
        assert_eq!(f.notes.len(), 1);
        assert!(f.notes[0].title.contains("cannot scan"));
        assert!(f.notes[0].details.contains("AAA: the key was refused"));
        a.last_error = f.errors[&a.id].clone();

        // Same failure later: written, not notified again.
        it.checked_at = Some(datetime!(2026-09-07 12:30:05 UTC));
        it.last_error = "the key was refused".into();
        run(&mut f, input(datetime!(2026-09-07 13:30:04 UTC), std::slice::from_ref(&a), &[it.clone()], &signals))
            .await
            .unwrap();
        assert_eq!(f.notes.len(), 1);
        a.last_error = f.errors[&a.id].clone();

        // The key works again: one recovery notice.
        f.refuse.clear();
        it.checked_at = Some(datetime!(2026-09-07 13:30:05 UTC));
        run(&mut f, input(datetime!(2026-09-07 14:30:04 UTC), std::slice::from_ref(&a), &[it], &signals))
            .await
            .unwrap();
        assert_eq!(f.notes.len(), 2, "{:?}", f.notes);
        assert!(f.notes[1].title.contains("scans again"));
        assert_eq!(f.errors[&a.id], "");
    }

    #[tokio::test]
    async fn scanner_job_hive_over_the_cap_is_an_error_not_a_scan() {
        let mut f = Fake::default();
        let signals: HashMap<Uuid, Result<Compiled, String>> =
            [compiled(2, "pattern", json!({"kind": "doji"}))].into_iter().collect();
        let alerts = vec![alert(1, vec![Uuid::from_u128(2)], "digest")];
        let why = store::cap_refusal(230, 200).unwrap();
        let input = PassInput {
            now: datetime!(2026-09-07 12:30:04 UTC),
            alerts: &alerts,
            targets: [(alerts[0].id, Err(why.clone()))].into_iter().collect(),
            signals: &signals,
            watched: vec![],
        };
        let r = run(&mut f, input).await.unwrap();
        assert_eq!(r.reads, 0);
        assert_eq!(f.notes.len(), 1);
        assert!(f.notes[0].details.contains("230 items"));
    }

    #[tokio::test]
    async fn scanner_job_a_board_item_without_an_alert_gets_its_cache() {
        let (mut f, items) = scripted();
        let watched = vec![items[0].clone()];
        let id = watched[0].id;
        let signals = HashMap::new();
        let input = PassInput { now: datetime!(2026-09-07 12:30:04 UTC), alerts: &[], targets: HashMap::new(), signals: &signals, watched };
        let r = run(&mut f, input).await.unwrap();
        assert_eq!(r.reads, 1);
        assert_eq!(r.hits, 0);
        assert!(f.caches.get(&id).is_some_and(|c| c.last_close.is_some()));
        assert!(f.notes.is_empty());
    }

    #[test]
    fn scanner_job_wakes_after_each_minute() {
        assert_eq!(until_next_wake(datetime!(2026-09-07 12:00:59.5 UTC)).as_millis(), 4_500);
        assert_eq!(until_next_wake(datetime!(2026-09-07 12:01:04 UTC)).as_millis(), 60_000);
    }

    #[test]
    fn scanner_job_an_item_is_read_once_its_next_bar_closed_then_backs_off() {
        let mut it = item(1, "X");
        let tf = 3600;
        assert!(item_due(&it, tf, datetime!(2026-09-07 12:00 UTC)));
        // Judged the 10:00 bar at 11:00:04.
        it.last_bar_ts = Some(datetime!(2026-09-07 10:00 UTC));
        it.checked_at = Some(datetime!(2026-09-07 11:00:04 UTC));
        assert!(!item_due(&it, tf, datetime!(2026-09-07 11:30:04 UTC)));
        // The 11:00 bar closed at 12:00: read at the first wake-up after.
        assert!(item_due(&it, tf, datetime!(2026-09-07 12:00:04 UTC)));
        // Not published yet at 12:00:04: retried after the bounded interval, not every minute.
        it.checked_at = Some(datetime!(2026-09-07 12:00:05 UTC));
        assert!(!item_due(&it, tf, datetime!(2026-09-07 12:01:04 UTC)));
        assert!(item_due(&it, tf, datetime!(2026-09-07 12:15:04 UTC)));
        // A failing item waits its retry too.
        it.last_error = "quota".into();
        assert!(!item_due(&it, tf, datetime!(2026-09-07 12:05:04 UTC)));
    }
}
