//! Chart alerts, evaluated server-side.
//!
//! The whole point is that an alert works with no browser open, so this is a background loop
//! over `histviz_alerts` and never a timer in the page.
//!
//! Three rules decide everything here:
//!
//! - **Closed bars only.** A forming candle's high is not a fact yet: it can be revised by
//!   the next tick, and an alert that fired on it would be reporting something that never
//!   happened. The evaluator drops any bar whose period has not ended.
//! - **A cross, not a state.** `above` means the series *crossed* the level going up (the
//!   previous closed bar was at or below it), not "is currently above". A level alert placed
//!   under the current price would otherwise fire the instant it is created, which is the one
//!   thing nobody wants from an alert.
//! - **The catalog first, the provider only for the edge.** A stored instrument costs nothing
//!   to watch. An unstored one costs one small request per cadence, which is why the cadence
//!   follows the timeframe rather than the loop's tick.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::backtest as bt_store;
use otw_store::crypto::SecretCipher;
use otw_store::histdata::{self as hist_store, Bar};
use otw_store::histviz::{self as store, Alert};
use otw_store::reminders::FiredNotification;

use crate::backtest::custom::{self, CustomIndicatorDef};
use crate::backtest::Bars;
use crate::notif_send;
use otw_store::connectors as conn_store;

/// How often the loop wakes. Each alert then decides whether *it* is due, from its own
/// timeframe: a daily alert re-read every minute would spend a provider request an hour for
/// a bar that moves once a day.
const TICK: Duration = Duration::from_secs(60);
/// Wait before the first pass, so a restart does not race the migrations and the connectors.
const WARMUP: Duration = Duration::from_secs(20);
/// Cadence bounds: no faster than the loop, no slower than a quarter hour, whatever the
/// timeframe. A 1w alert still gets looked at often enough to be useful the day it fires.
const MIN_CADENCE: i64 = 60;
const MAX_CADENCE: i64 = 900;
/// Bars asked for beyond an indicator's own lookback, so the series is warm and the previous
/// closed bar (the one a cross is measured against) is always there.
const EXTRA_BARS: i64 = 5;
/// Ceiling on the window one evaluation may ask a provider for.
const MAX_BARS: i64 = 400;

pub struct AlertCx {
    pub cipher: SecretCipher,
    pub http: Client,
}

pub fn spawn(pool: PgPool, cx: AlertCx) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = pass(&pool, &cx).await {
                tracing::error!("chart alerts: pass failed: {e:#}");
            }
        }
    });
}

/// One sweep: every enabled alert whose cadence has elapsed.
async fn pass(pool: &PgPool, cx: &AlertCx) -> Result<()> {
    let now = OffsetDateTime::now_utc();
    // The store filter is the coarse one (the loop's own period); each alert's real cadence
    // is its timeframe, applied here where the timeframe is known.
    let stale = now - time::Duration::seconds(MIN_CADENCE - 5);
    let due = store::due_alerts(pool, stale).await?;
    for a in due {
        if !is_due(&a, now) {
            continue;
        }
        if let Err(e) = evaluate(pool, cx, &a).await {
            let msg = short(&format!("{e:#}"));
            tracing::warn!("chart alert {} ({}): {msg}", a.id, a.ticker);
            let _ = store::mark_error(pool, a.id, &msg).await;
        }
    }
    Ok(())
}

/// Whether this alert's own cadence has elapsed.
fn is_due(a: &Alert, now: OffsetDateTime) -> bool {
    let Some(last) = a.checked_at else { return true };
    (now - last).whole_seconds() >= cadence(&a.timeframe)
}

/// How often an alert on this timeframe is worth re-reading: the bar length, bounded.
fn cadence(timeframe: &str) -> i64 {
    let tf = crate::histdata::timeframe_secs(timeframe).unwrap_or(3600);
    tf.clamp(MIN_CADENCE, MAX_CADENCE)
}

/// Evaluate one alert: read its bars, build its series, judge the newest closed bar.
async fn evaluate(pool: &PgPool, cx: &AlertCx, a: &Alert) -> Result<()> {
    let tf = crate::histdata::timeframe_secs(&a.timeframe)
        .map_err(|e| anyhow!("{e}"))?;
    let def = match (a.kind.as_str(), a.indicator_id) {
        ("indicator", Some(id)) => Some(indicator_def(pool, id).await?),
        ("indicator", None) => return Err(anyhow!("this alert has no indicator")),
        _ => None,
    };
    let need = def.as_ref().map(|d| d.lookback() as i64).unwrap_or(0) + EXTRA_BARS;
    let want = need.clamp(EXTRA_BARS, MAX_BARS);

    let bars = load_bars(pool, cx, a, tf, want).await?;
    let closed = closed_bars(&bars, tf, OffsetDateTime::now_utc());
    if closed.len() < 2 {
        // Not enough history to say anything about a crossing yet.
        store::mark_checked(pool, a.id, None, None).await?;
        return Ok(());
    }
    let series = series_for(a, def.as_ref(), &closed);
    let n = series.len();
    let (prev, cur) = (series[n - 2], series[n - 1]);
    let bar_ts = closed[n - 1].ts;
    let (Some(prev), Some(cur)) = (prev, cur) else {
        store::mark_checked(pool, a.id, Some(bar_ts), None).await?;
        return Ok(());
    };

    // The same bar is judged once: the loop wakes more often than a bar closes.
    if a.last_bar_ts.is_some_and(|t| t >= bar_ts) {
        store::mark_checked(pool, a.id, None, Some(cur)).await?;
        return Ok(());
    }
    if !crossed(&a.op, prev, cur, a.value) || cooling_down(a, OffsetDateTime::now_utc()) {
        store::mark_checked(pool, a.id, Some(bar_ts), Some(cur)).await?;
        return Ok(());
    }
    fire(pool, cx, a, cur, bar_ts, def.as_ref()).await
}

/// Bars for the window this alert needs: the catalog first, the provider only when the
/// catalog cannot reach the present.
async fn load_bars(
    pool: &PgPool,
    cx: &AlertCx,
    a: &Alert,
    tf: i64,
    want: i64,
) -> Result<Vec<Bar>> {
    let now = OffsetDateTime::now_utc();
    let from = now - time::Duration::seconds(tf * (want + 1));
    let dataset =
        hist_store::find_dataset(pool, &a.provider, &a.asset_type, &a.ticker, &a.timeframe).await?;
    let mut rows = match &dataset {
        Some(d) => hist_store::read_bars(pool, d.id, Some(from), Some(now), want + 2).await?,
        None => Vec::new(),
    };
    // Fresh enough to answer on its own: the stored series already covers the last closed bar.
    let fresh = rows
        .last()
        .is_some_and(|b| (now - b.ts).whole_seconds() < tf * 2);
    if !fresh {
        let connector = resolve_connector(pool, a).await?;
        let secrets = conn_store::load_creds(pool, &cx.cipher, connector).await?;
        let rest = crate::histdata::connector_for(&a.provider)?;
        let start = rows
            .last()
            .map(|b| b.ts)
            .unwrap_or(from)
            .max(now - time::Duration::seconds(tf * (MAX_BARS + 1)));
        let chunk = rest
            .fetch_chunk(&cx.http, &secrets, &a.ticker, &a.asset_type, &a.timeframe, start, now)
            .await
            .context("reading bars for the alert")?;
        rows.extend(chunk.bars);
        // Watching is not downloading: an alert stores nothing, exactly like the chart it was
        // drawn on.
        rows.sort_by_key(|b| b.ts);
        rows.dedup_by_key(|b| b.ts);
    }
    Ok(rows)
}

/// The connector this alert reads through: its own, else the oldest one the chart is granted
/// for that provider. Resolved at evaluation time, so a connector deleted or re-granted
/// yesterday does not orphan an alert.
async fn resolve_connector(pool: &PgPool, a: &Alert) -> Result<Uuid> {
    if let Some(id) = a.connector_id {
        if let Some(c) = conn_store::get(pool, id).await? {
            if c.allows("histviz") {
                return Ok(c.id);
            }
        }
    }
    conn_store::default_for_module(pool, &a.provider, "histviz")
        .await?
        .map(|c| c.id)
        .ok_or_else(|| {
            anyhow!(
                "no {} connector is granted to the chart, so this alert cannot read its bars",
                a.provider
            )
        })
}

/// Bars whose period has ended. A forming candle is not evidence.
fn closed_bars(bars: &[Bar], tf: i64, now: OffsetDateTime) -> Vec<Bar> {
    bars.iter()
        .filter(|b| b.ts.unix_timestamp() + tf <= now.unix_timestamp())
        .cloned()
        .collect()
}

/// The series the level is compared against: a price, or the indicator's own output.
fn series_for(a: &Alert, def: Option<&CustomIndicatorDef>, bars: &[Bar]) -> Vec<Option<f64>> {
    match def {
        Some(def) => {
            let ts: Vec<String> = bars.iter().map(|b| b.ts.to_string()).collect();
            let open: Vec<f64> = bars.iter().map(|b| b.open).collect();
            let high: Vec<f64> = bars.iter().map(|b| b.high).collect();
            let low: Vec<f64> = bars.iter().map(|b| b.low).collect();
            let close: Vec<f64> = bars.iter().map(|b| b.close).collect();
            let volume: Vec<f64> = bars.iter().map(|b| b.volume).collect();
            let b = Bars {
                ticker: &a.ticker,
                ts: &ts,
                open: &open,
                high: &high,
                low: &low,
                close: &close,
                volume: &volume,
            };
            custom::eval(def, &b)
        }
        None => bars
            .iter()
            .map(|b| {
                Some(match a.source.as_str() {
                    "high" => b.high,
                    "low" => b.low,
                    "open" => b.open,
                    _ => b.close,
                })
            })
            .collect(),
    }
}

/// Did the series cross the level between these two closed bars?
///
/// A crossing, never a state: `above` needs the previous bar at or below the level. An alert
/// placed under the price would otherwise fire on the very next evaluation, which is a
/// notification about the past.
pub fn crossed(op: &str, prev: f64, cur: f64, level: f64) -> bool {
    let up = prev <= level && cur > level;
    let down = prev >= level && cur < level;
    match op {
        "above" => up,
        "below" => down,
        _ => up || down,
    }
}

fn cooling_down(a: &Alert, now: OffsetDateTime) -> bool {
    match (a.last_fired_at, a.cooldown_secs) {
        (Some(t), secs) if secs > 0 => (now - t).whole_seconds() < secs as i64,
        _ => false,
    }
}

async fn indicator_def(pool: &PgPool, id: Uuid) -> Result<CustomIndicatorDef> {
    let row = bt_store::get_indicator(pool, id)
        .await?
        .ok_or_else(|| anyhow!("the indicator this alert watches no longer exists"))?;
    let def: CustomIndicatorDef = serde_json::from_value(row.definition)
        .with_context(|| format!("reading the definition of {}", row.name))?;
    if let Some(why) = def.validate() {
        return Err(anyhow!("{} is not a valid indicator: {why}", row.name));
    }
    Ok(def)
}

/// Write the in-app notification, push it to the alert's channels, and record the fire.
async fn fire(
    pool: &PgPool,
    cx: &AlertCx,
    a: &Alert,
    value: f64,
    bar_ts: OffsetDateTime,
    def: Option<&CustomIndicatorDef>,
) -> Result<()> {
    let subject = match (a.kind.as_str(), def) {
        ("indicator", _) => a.name.clone(),
        _ => format!("{} {}", a.ticker, a.source),
    };
    let title = if a.name.trim().is_empty() {
        format!("{} {} {}", a.ticker, verb(&a.op), fmt(a.value))
    } else {
        a.name.trim().to_string()
    };
    let details = format!(
        "{subject} {} {} on the {} bar of {} that closed at {}. Now {}.",
        verb(&a.op),
        fmt(a.value),
        a.timeframe,
        a.ticker,
        stamp(bar_ts),
        fmt(value)
    );
    let url = format!("/histviz?alert={}", a.id);

    sqlx::query(
        "INSERT INTO notifications (id, name, kind, linked_id, details, url, link_label) \
         VALUES ($1,$2,'chart',$3,$4,$5,$6)",
    )
    .bind(Uuid::new_v4())
    .bind(&title)
    .bind(a.id)
    .bind(&details)
    .bind(&url)
    .bind(&a.ticker)
    .execute(pool)
    .await?;

    let channels: Vec<Uuid> = a
        .channels
        .as_array()
        .map(|v| v.iter().filter_map(|x| x.as_str()).filter_map(|s| Uuid::parse_str(s).ok()).collect())
        .unwrap_or_default();
    let notif = FiredNotification { name: title, details, url };
    notif_send::dispatch(
        pool,
        &cx.cipher,
        &cx.http,
        &notif,
        "histviz",
        // An empty list means "every channel the chart is granted", the same convention the
        // rest of the broker uses; a non-empty one is the user's own narrowing.
        if channels.is_empty() { None } else { Some(&channels) },
    )
    .await;
    store::record_fire(pool, a.id, value, bar_ts, a.repeat).await?;
    Ok(())
}

/// The bar's own time, written the way a person reads it. RFC3339 would be exact and
/// unreadable in a push notification, which is where this line ends up.
fn stamp(ts: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        ts.year(),
        ts.month() as u8,
        ts.day(),
        ts.hour(),
        ts.minute()
    )
}

fn verb(op: &str) -> &'static str {
    match op {
        "above" => "crossed above",
        "below" => "crossed below",
        _ => "crossed",
    }
}

/// A price the way the chart writes it: enough digits for a micro-cap, none wasted on an index.
fn fmt(v: f64) -> String {
    let abs = v.abs();
    let digits = if abs >= 1000.0 {
        2
    } else if abs >= 1.0 {
        4
    } else {
        8
    };
    let s = format!("{v:.digits$}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// One line of the reason, for the row the user reads.
fn short(msg: &str) -> String {
    let first = msg.split('\n').next().unwrap_or(msg).trim();
    let mut out: String = first.chars().take(200).collect();
    if first.chars().count() > 200 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn bar(ts: OffsetDateTime, close: f64) -> Bar {
        Bar {
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
        }
    }

    #[test]
    fn a_level_fires_on_the_crossing_and_not_on_the_state() {
        // Already above when the alert is placed: nothing crossed, nothing fires.
        assert!(!crossed("above", 105.0, 106.0, 100.0));
        // The bar that actually crosses it does.
        assert!(crossed("above", 99.0, 101.0, 100.0));
        // Touching the level from below counts as still below, so the next bar can cross it.
        assert!(crossed("above", 100.0, 101.0, 100.0));
        assert!(!crossed("above", 101.0, 99.0, 100.0));
        assert!(crossed("below", 101.0, 99.0, 100.0));
        assert!(crossed("crosses", 101.0, 99.0, 100.0));
        assert!(crossed("crosses", 99.0, 101.0, 100.0));
        assert!(!crossed("crosses", 98.0, 99.0, 100.0));
    }

    #[test]
    fn the_message_says_which_bar_closed() {
        assert_eq!(stamp(datetime!(2026-09-06 09:22:00 UTC)), "2026-09-06 09:22 UTC");
    }

    #[test]
    fn a_forming_bar_is_not_evidence() {
        let now = datetime!(2026-09-06 12:30:20 UTC);
        let bars = vec![
            bar(datetime!(2026-09-06 12:00:00 UTC), 10.0),
            bar(datetime!(2026-09-06 12:15:00 UTC), 11.0),
            // The 12:30 bar of a 15m series has 14 minutes left to run.
            bar(datetime!(2026-09-06 12:30:00 UTC), 99.0),
        ];
        let closed = closed_bars(&bars, 900, now);
        assert_eq!(closed.len(), 2);
        assert_eq!(closed.last().unwrap().close, 11.0);
    }

    #[test]
    fn the_cadence_follows_the_timeframe_within_bounds() {
        assert_eq!(cadence("1m"), 60);
        assert_eq!(cadence("5m"), 300);
        // A daily alert is not re-read every minute, and not once a day either.
        assert_eq!(cadence("1d"), MAX_CADENCE);
        assert_eq!(cadence("nonsense"), 900);
    }

    #[test]
    fn a_price_alert_reads_the_source_it_was_given() {
        let mut a = Alert {
            id: Uuid::new_v4(),
            name: String::new(),
            provider: "binance".into(),
            asset_type: "crypto".into(),
            ticker: "BTCUSDT".into(),
            timeframe: "1h".into(),
            connector_id: None,
            kind: "price".into(),
            source: "high".into(),
            indicator_id: None,
            op: "above".into(),
            value: 10.0,
            channels: serde_json::json!([]),
            repeat: false,
            cooldown_secs: 0,
            enabled: true,
            last_bar_ts: None,
            last_value: None,
            last_fired_at: None,
            fire_count: 0,
            checked_at: None,
            last_error: String::new(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let mut b = bar(datetime!(2026-09-06 12:00:00 UTC), 10.0);
        b.high = 12.0;
        b.low = 8.0;
        assert_eq!(series_for(&a, None, &[b.clone()]), vec![Some(12.0)]);
        a.source = "low".into();
        assert_eq!(series_for(&a, None, &[b.clone()]), vec![Some(8.0)]);
        a.source = "close".into();
        assert_eq!(series_for(&a, None, &[b]), vec![Some(10.0)]);
    }
}
