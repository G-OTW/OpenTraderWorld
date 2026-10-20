//! Market sessions (Settings → Sessions): named windows in a venue's timezone, used by chart
//! alerts that fire "once per session".
//!
//! - `GET/POST        /api/market-sessions`       the list (plus the timezone names), create
//! - `PUT/DELETE      /api/market-sessions/{id}`  replace, delete
//!
//! A session is stored as local wall-clock minutes in a timezone, never as UTC offsets, so a
//! DST switch moves it with the market. [`occurrence`] turns it into the instant range
//! running at a given moment.

use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::{Duration, OffsetDateTime};
use time_tz::OffsetDateTimeExt;
use uuid::Uuid;

use crate::automator::schedule;
use crate::{ApiError, AppState};
use otw_store::market_sessions::{self as store, Session, SessionInput};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/market-sessions", get(list).post(create))
        .route("/api/market-sessions/{id}", put(update).delete(remove))
}

/// The occurrence of `s` running at `now`, as `[start, end)`, or `None` outside it.
///
/// A window opens on the local day whose weekday bit is set, so an overnight session that
/// opens Friday evening still runs into Saturday morning, and one that opens Sunday evening is
/// a Sunday session. Only today's and yesterday's openings can contain `now`, since a window is
/// shorter than a day.
pub fn occurrence(s: &Session, now: OffsetDateTime) -> Option<(OffsetDateTime, OffsetDateTime)> {
    let tz = schedule::timezone(&s.timezone)?;
    let today = now.to_timezone(tz).date();
    let (sh, sm) = ((s.start_minute / 60) as u8, (s.start_minute % 60) as u8);
    let (eh, em) = ((s.end_minute / 60) as u8, (s.end_minute % 60) as u8);
    for day in [today - Duration::days(1), today] {
        if s.weekdays & (1 << schedule::weekday_index(day.weekday())) == 0 {
            continue;
        }
        let end_day = if s.end_minute <= s.start_minute { day + Duration::days(1) } else { day };
        let (Some(start), Some(end)) =
            (schedule::at_local(tz, day, sh, sm), schedule::at_local(tz, end_day, eh, em))
        else {
            continue;
        };
        if start <= now && now < end {
            return Some((start, end));
        }
    }
    None
}

#[derive(Deserialize)]
struct Body {
    name: String,
    timezone: String,
    start_minute: i32,
    end_minute: i32,
    weekdays: i32,
}

impl Body {
    fn input(&self) -> Result<SessionInput<'_>, ApiError> {
        if self.name.trim().is_empty() {
            return Err(ApiError::bad_request("name is required"));
        }
        if schedule::timezone(self.timezone.trim()).is_none() {
            return Err(ApiError::bad_request(&format!(
                "unknown timezone \"{}\"",
                self.timezone.trim()
            )));
        }
        for (what, v) in [("start", self.start_minute), ("end", self.end_minute)] {
            if !(0..1440).contains(&v) {
                return Err(ApiError::bad_request(&format!("{what} must be a time of day")));
            }
        }
        if self.start_minute == self.end_minute {
            return Err(ApiError::bad_request("a session cannot start and end at the same time"));
        }
        if !(1..=127).contains(&self.weekdays) {
            return Err(ApiError::bad_request("pick at least one day of the week"));
        }
        Ok(SessionInput {
            name: self.name.trim(),
            timezone: self.timezone.trim(),
            start_minute: self.start_minute,
            end_minute: self.end_minute,
            weekdays: self.weekdays,
        })
    }
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sessions = store::list(&state.pool).await?;
    Ok(Json(json!({ "sessions": sessions, "timezones": schedule::timezone_names() })))
}

async fn create(
    State(state): State<AppState>,
    Json(b): Json<Body>,
) -> Result<Json<Value>, ApiError> {
    let s = store::create(&state.pool, b.input()?).await?;
    Ok(Json(json!(s)))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<Body>,
) -> Result<Json<Value>, ApiError> {
    let s = store::update(&state.pool, id, b.input()?)
        .await?
        .ok_or_else(|| ApiError::not_found("session"))?;
    Ok(Json(json!(s)))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete(&state.pool, id).await? {
        return Err(ApiError::not_found("session"));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn session(tz: &str, start: i32, end: i32, weekdays: i32) -> Session {
        Session {
            id: Uuid::new_v4(),
            name: "s".into(),
            timezone: tz.into(),
            start_minute: start,
            end_minute: end,
            weekdays,
            position: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn the_us_session_follows_new_york_across_dst() {
        let us = session("America/New_York", 570, 960, 31);
        // Summer: 09:30 EDT is 13:30 UTC.
        let (start, end) = occurrence(&us, datetime!(2026-07-08 14:00 UTC)).unwrap();
        assert_eq!(start, datetime!(2026-07-08 13:30 UTC));
        assert_eq!(end, datetime!(2026-07-08 20:00 UTC));
        // Winter: 09:30 EST is 14:30 UTC, so 14:00 UTC is still pre-market.
        assert!(occurrence(&us, datetime!(2026-01-07 14:00 UTC)).is_none());
        assert!(occurrence(&us, datetime!(2026-01-07 14:31 UTC)).is_some());
    }

    #[test]
    fn a_weekend_is_outside_a_weekday_session() {
        let us = session("America/New_York", 570, 960, 31);
        assert!(occurrence(&us, datetime!(2026-07-11 15:00 UTC)).is_none());
    }

    #[test]
    fn an_overnight_session_belongs_to_the_day_it_opens() {
        // 22:00 to 06:00 UTC, opening Monday through Friday.
        let night = session("UTC", 1320, 360, 31);
        let (start, _) = occurrence(&night, datetime!(2026-07-11 03:00 UTC)).unwrap();
        // Saturday 03:00 is still Friday's session.
        assert_eq!(start, datetime!(2026-07-10 22:00 UTC));
        // Sunday 03:00 would be Saturday's, which does not open.
        assert!(occurrence(&night, datetime!(2026-07-12 03:00 UTC)).is_none());
    }
}
