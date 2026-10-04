//! Exchange calendars: regular hours, lunch breaks, holidays and half days per venue, and
//! whether one is trading right now.
//!
//! The data is reference data, not the user's: `calendars.json` is generated from
//! `exchange_calendars` by `.github/exchange-calendars/generate.py` and embedded in the
//! binary, so a fresh install knows its holidays offline. Once a week the server fetches the
//! copy a scheduled workflow (`.github/workflows/exchange-calendars.yml`) regenerates and
//! publishes as a release asset, the same way FinanceDatabase ships, and adopts it when it is
//! newer: that is how an install learns next year's holidays without an update. The adopted copy is kept in
//! `app_settings` so a restart does not fall back to the older embedded one.
//!
//! Hours are local wall-clock minutes in the venue's timezone, so DST is the timezone
//! database's business. A session is labelled by its trading date; a venue whose session
//! opens the evening before (CME Globex) says so with `prior_day_open`.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};
use time_tz::OffsetDateTimeExt;

use crate::automator::schedule;

const EMBEDDED: &str = include_str!("calendars.json");
/// Where the weekly sync reads the newest snapshot.
const REMOTE: &str =
    "https://github.com/G-OTW/OpenTraderWorld/releases/download/exchange-calendars/calendars.json";
const KEY_BODY: &str = "exchange_calendar";
const KEY_CHECKED: &str = "exchange_calendar_checked_at";
/// How often the remote copy is looked at.
const SYNC_EVERY: i64 = 7 * 24 * 3600;
/// Loop period: cheap (one settings read), and short enough that a server that was off on
/// sync day catches up within the hour.
const TICK: Duration = Duration::from_secs(3600);
const WARMUP: Duration = Duration::from_secs(30);
/// Ceiling on the remote file, which is ~35 kB today.
const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calendar {
    /// RFC3339; newer wins.
    pub generated: String,
    pub source: String,
    pub from: String,
    pub to: String,
    pub exchanges: Vec<Exchange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exchange {
    pub mic: String,
    /// Three letters for the top bar.
    pub code: String,
    pub name: String,
    pub city: String,
    pub timezone: String,
    pub open: i32,
    pub close: i32,
    pub break_start: Option<i32>,
    pub break_end: Option<i32>,
    pub prior_day_open: bool,
    /// Session labels that trade, Monday = bit 0.
    pub weekdays: i32,
    /// Weekday labels that do not trade (holidays), `YYYY-MM-DD`.
    pub closed: Vec<String>,
    /// Labels whose open or close differs from the regular hours (half days).
    pub special: Vec<Special>,
    /// Last date the holidays are known for.
    pub covered_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Special {
    pub date: String,
    pub open: Option<i32>,
    pub close: Option<i32>,
}

/// Whether a venue trades at a given instant, and when that changes.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Status {
    pub open: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_change: Option<OffsetDateTime>,
    /// Past `covered_to`: the regular hours still apply, the holidays are not known.
    pub holidays_unknown: bool,
}

fn iso(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

impl Exchange {
    /// The trading intervals of the session labelled `label`, split by the lunch break.
    /// Empty on a weekend or a holiday.
    pub fn intervals(&self, label: Date) -> Vec<(OffsetDateTime, OffsetDateTime)> {
        let Some(tz) = schedule::timezone(&self.timezone) else { return Vec::new() };
        if self.weekdays & (1 << schedule::weekday_index(label.weekday())) == 0 {
            return Vec::new();
        }
        let day = iso(label);
        if self.closed.binary_search(&day).is_ok() {
            return Vec::new();
        }
        let special = self.special.iter().find(|s| s.date == day);
        let open = special.and_then(|s| s.open).unwrap_or(self.open);
        let close = special.and_then(|s| s.close).unwrap_or(self.close);
        let open_day = if self.prior_day_open { label.previous_day() } else { Some(label) };
        let at = |d: Date, m: i32| schedule::at_local(tz, d, (m / 60) as u8, (m % 60) as u8);
        let (Some(start), Some(end)) = (open_day.and_then(|d| at(d, open)), at(label, close))
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        match (self.break_start, self.break_end, self.prior_day_open) {
            (Some(bs), Some(be), false) => {
                if let (Some(b0), Some(b1)) = (at(label, bs), at(label, be)) {
                    out.push((start, b0.min(end)));
                    out.push((b1.max(start), end));
                } else {
                    out.push((start, end));
                }
            }
            _ => out.push((start, end)),
        }
        out.retain(|(a, b)| a < b);
        out
    }

    pub fn status(&self, now: OffsetDateTime) -> Status {
        let Some(tz) = schedule::timezone(&self.timezone) else {
            return Status { open: false, next_change: None, holidays_unknown: true };
        };
        let today = now.to_timezone(tz).date();
        let holidays_unknown = iso(today) > self.covered_to;
        // Yesterday's label can still be running (an overnight session), tomorrow's can have
        // opened already (a session that opens the evening before).
        let mut label = today.previous_day().unwrap_or(today);
        let mut next_open: Option<OffsetDateTime> = None;
        for _ in 0..16 {
            for (a, b) in self.intervals(label) {
                if a <= now && now < b {
                    return Status { open: true, next_change: Some(b), holidays_unknown };
                }
                if a > now && next_open.is_none_or(|n| a < n) {
                    next_open = Some(a);
                }
            }
            if next_open.is_some() && label > today {
                break;
            }
            let Some(next) = label.next_day() else { break };
            label = next;
        }
        Status { open: false, next_change: next_open, holidays_unknown }
    }

    /// Holidays and half days from `from` on, at most `limit`.
    pub fn upcoming(&self, from: Date, limit: usize) -> Vec<Special> {
        let from = iso(from);
        let mut out: Vec<Special> = self
            .closed
            .iter()
            .filter(|d| **d >= from)
            .map(|d| Special { date: d.clone(), open: None, close: None })
            .chain(self.special.iter().filter(|s| s.date >= from).cloned())
            .collect();
        out.sort_by(|a, b| a.date.cmp(&b.date));
        out.truncate(limit);
        out
    }
}

impl Calendar {
    fn parse(body: &str) -> Result<Self> {
        let mut c: Calendar = serde_json::from_str(body).context("reading the exchange calendar")?;
        if c.exchanges.is_empty() {
            bail!("the exchange calendar lists no exchange");
        }
        OffsetDateTime::parse(&c.generated, &Rfc3339).context("calendar generation date")?;
        for e in &mut c.exchanges {
            // `intervals` looks holidays up by binary search.
            e.closed.sort();
        }
        Ok(c)
    }

    pub fn find(&self, mic: &str) -> Option<&Exchange> {
        self.exchanges.iter().find(|e| e.mic == mic)
    }

    fn newer_than(&self, other: &Calendar) -> bool {
        let at = |s: &str| OffsetDateTime::parse(s, &Rfc3339).ok();
        match (at(&self.generated), at(&other.generated)) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// Shared handle: the calendar in use, swapped whole when a newer one arrives.
#[derive(Clone)]
pub struct Exchanges(Arc<RwLock<Arc<Calendar>>>);

impl Exchanges {
    pub fn current(&self) -> Arc<Calendar> {
        self.0.read().expect("calendar lock").clone()
    }

    fn adopt(&self, c: Calendar) {
        *self.0.write().expect("calendar lock") = Arc::new(c);
    }
}

/// The embedded calendar, or the stored one when a sync left a newer one behind.
pub async fn load(pool: &PgPool) -> Exchanges {
    let embedded = Calendar::parse(EMBEDDED).expect("the embedded exchange calendar is valid");
    let stored = otw_store::settings::get(pool, KEY_BODY)
        .await
        .ok()
        .flatten()
        .and_then(|b| Calendar::parse(&b).ok());
    let pick = match stored {
        Some(s) if s.newer_than(&embedded) => s,
        _ => embedded,
    };
    Exchanges(Arc::new(RwLock::new(Arc::new(pick))))
}

/// Weekly: fetch the published calendar and adopt it when newer.
pub fn spawn(pool: PgPool, http: reqwest::Client, ex: Exchanges) {
    tokio::spawn(async move {
        tokio::time::sleep(WARMUP).await;
        loop {
            if due(&pool).await {
                if let Err(e) = sync(&pool, &http, &ex).await {
                    tracing::warn!("exchange calendar sync failed: {e:#}");
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

async fn due(pool: &PgPool) -> bool {
    let last = otw_store::settings::get(pool, KEY_CHECKED)
        .await
        .ok()
        .flatten()
        .and_then(|s| OffsetDateTime::parse(&s, &Rfc3339).ok());
    last.is_none_or(|t| (OffsetDateTime::now_utc() - t).whole_seconds() >= SYNC_EVERY)
}

/// Fetch once. True = a newer calendar was adopted.
pub async fn sync(pool: &PgPool, http: &reqwest::Client, ex: &Exchanges) -> Result<bool> {
    let now = OffsetDateTime::now_utc().format(&Rfc3339)?;
    // Written first: a remote that keeps failing is retried next week, not every hour.
    otw_store::settings::set(pool, KEY_CHECKED, &now).await?;
    let res = http
        .get(REMOTE)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .context("fetching the exchange calendar")?
        .error_for_status()
        .context("fetching the exchange calendar")?;
    let bytes = res.bytes().await.context("reading the exchange calendar")?;
    if bytes.len() > MAX_BYTES {
        bail!("the published exchange calendar is larger than expected");
    }
    let body = std::str::from_utf8(&bytes).context("the exchange calendar is not text")?;
    let remote = Calendar::parse(body)?;
    if !remote.newer_than(&ex.current()) {
        return Ok(false);
    }
    otw_store::settings::set(pool, KEY_BODY, body).await?;
    tracing::info!("exchange calendar updated to {}", remote.generated);
    ex.adopt(remote);
    Ok(true)
}

/// When the remote copy was last looked at.
pub async fn checked_at(pool: &PgPool) -> Option<String> {
    otw_store::settings::get(pool, KEY_CHECKED).await.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    fn cal() -> Calendar {
        Calendar::parse(EMBEDDED).unwrap()
    }

    #[test]
    fn the_embedded_calendar_parses_and_names_three_letters() {
        let c = cal();
        assert!(c.exchanges.len() >= 20);
        for e in &c.exchanges {
            assert_eq!(e.code.len(), 3, "{}", e.mic);
            assert!(schedule::timezone(&e.timezone).is_some(), "{}", e.timezone);
        }
    }

    #[test]
    fn nyse_is_open_in_the_afternoon_and_closed_on_a_holiday() {
        let c = cal();
        let nyse = c.find("XNYS").unwrap();
        // Wednesday 2026-07-08, 11:00 New York.
        let s = nyse.status(datetime!(2026-07-08 15:00 UTC));
        assert!(s.open);
        assert_eq!(s.next_change, Some(datetime!(2026-07-08 20:00 UTC)));
        // Friday 2026-07-03, Independence Day observed.
        let s = nyse.status(datetime!(2026-07-03 15:00 UTC));
        assert!(!s.open);
        assert_eq!(s.next_change, Some(datetime!(2026-07-06 13:30 UTC)));
    }

    #[test]
    fn a_half_day_closes_early() {
        let c = cal();
        let nyse = c.find("XNYS").unwrap();
        // Day after Thanksgiving 2026: 13:00 New York close.
        assert!(nyse.status(datetime!(2026-11-27 17:30 UTC)).open);
        assert!(!nyse.status(datetime!(2026-11-27 18:30 UTC)).open);
    }

    #[test]
    fn tokyo_breaks_for_lunch() {
        let c = cal();
        let tyo = c.find("XTKS").unwrap();
        assert_eq!(tyo.intervals(date!(2026 - 07 - 08)).len(), 2);
        // 12:00 Tokyo = 03:00 UTC: lunch.
        let s = tyo.status(datetime!(2026-07-08 03:00 UTC));
        assert!(!s.open);
        assert_eq!(s.next_change, Some(datetime!(2026-07-08 03:30 UTC)));
    }

    #[test]
    fn cme_opens_the_evening_before_and_halts_an_hour() {
        let c = cal();
        let cme = c.find("XCME").unwrap();
        // Sunday 2026-07-12 18:00 Chicago (23:00 UTC): Monday's session is running.
        assert!(cme.status(datetime!(2026-07-12 23:00 UTC)).open);
        // Tuesday 16:30 Chicago (21:30 UTC): the maintenance hour.
        let s = cme.status(datetime!(2026-07-14 21:30 UTC));
        assert!(!s.open);
        assert_eq!(s.next_change, Some(datetime!(2026-07-14 22:00 UTC)));
        // Saturday: closed until Sunday evening.
        let s = cme.status(datetime!(2026-07-11 12:00 UTC));
        assert!(!s.open);
        assert_eq!(s.next_change, Some(datetime!(2026-07-12 22:00 UTC)));
    }

    #[test]
    fn the_newest_generation_wins() {
        let a = cal();
        let mut b = a.clone();
        b.generated = "2099-01-01T00:00:00Z".into();
        assert!(b.newer_than(&a));
        assert!(!a.newer_than(&b));
    }
}
