//! Custom value sources as a Fundamentals provider.
//!
//! A custom connector whose shape is `value` (`histdata/kinds/value.rs`) serves one series per
//! symbol: the `http` kind pulls it (`fetch`), the `push` kind receives it from a script
//! through the webhook, the file import reads it from a file. Every way in lands in
//! `fund_series` + `fund_observations` with [`store_observations`], which upserts by period
//! (latest vintage wins) and tells the live charts drawing the series through the hub
//! (`point` events on `fund:<series_id>`).
//!
//! The series is placed on a chart, and fed to an indicator, at its release time when the
//! source gives one, else at the end of its period ([`points`]): a value is never read before
//! it was published, and without a release date the period being over is the earliest it can
//! have been.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use sqlx::PgPool;
use uuid::Uuid;

use crate::histdata::kinds::{self, value::Obs};
use otw_store::fundamentals::{self as store, Observation, SeriesMeta};

/// Live points sent per write at most: a first fetch of decades of history is not news.
const LIVE_POINTS: usize = 50;

/// Whether `provider` is a custom value source.
pub fn serves(provider: &str) -> bool {
    kinds::serves_values(provider)
}

fn label(provider: &str) -> String {
    crate::histdata::connector_for(provider)
        .map(|c| c.capability().label.to_string())
        .unwrap_or_else(|_| provider.to_string())
}

/// What a custom series is, from its connector's settings: its unit and frequency.
pub fn meta(provider: &str, code: &str, title: Option<&str>) -> Result<SeriesMeta> {
    let vm = kinds::value_mapping(provider)
        .ok_or_else(|| anyhow!("{provider} is not a custom value source: set its shape to value"))?;
    Ok(SeriesMeta {
        provider: provider.to_string(),
        code: code.to_string(),
        title: title.map(str::to_string).unwrap_or_else(|| code.to_string()),
        category: "other".into(),
        country: String::new(),
        unit: vm.unit,
        frequency: vm.frequency.to_string(),
        seasonal_adj: false,
        notes: format!("Custom source {}", label(provider)),
    })
}

/// The source's series: its static symbol list or its symbols URL, filtered by `q`.
pub async fn search(provider: &str, client: &reqwest::Client, secrets: &HashMap<String, String>, q: &str) -> Result<Vec<SeriesMeta>> {
    let c = crate::histdata::connector_for(provider)?;
    let hits = c.search_symbols(client, secrets, q, "", 50).await?;
    hits.into_iter()
        .map(|h| meta(provider, &h.symbol, (!h.name.is_empty()).then_some(h.name.as_str())))
        .collect()
}

/// The observations of one series from `since` (its last stored period; none = the default
/// history start) to now, pulled through an `http` value source page by page, each request
/// counted on `meter`, with how many periods came without a value. A push source has nothing
/// to fetch and says so.
pub async fn fetch(
    provider: &str,
    secrets: &HashMap<String, String>,
    code: &str,
    since: Option<time::Date>,
    meter: Option<&kinds::http::Meter>,
) -> Result<(SeriesMeta, Vec<Obs>, usize)> {
    match kinds::kind_of(provider) {
        Some(kinds::http::KIND) => {
            let (obs, missing) = kinds::http_instance(provider)?.fetch_observations(secrets, code, since, meter).await?;
            Ok((meta(provider, code, None)?, obs, missing))
        }
        Some(kinds::push::KIND) => {
            bail!("{} is a push source: {code} arrives through its webhook, there is nothing to fetch", label(provider))
        }
        _ => bail!("{provider} is not a custom value source"),
    }
}

/// One request for a series, to check its code before it is stored. A push source's series
/// is created by its first push, so there is nothing to check.
pub async fn probe(provider: &str, secrets: &HashMap<String, String>, code: &str) -> Result<SeriesMeta> {
    match kinds::kind_of(provider) {
        Some(kinds::http::KIND) => {
            kinds::http_instance(provider)?.probe_observations(secrets, code).await?;
            meta(provider, code, None)
        }
        _ => fetch(provider, secrets, code, None, None).await.map(|(m, _, _)| m),
    }
}

/// Store observations of a series (upsert by period, latest vintage wins) and tell the live
/// charts about the new or revised ones. Returns how many changed.
pub async fn store_observations(pool: &PgPool, hub: &crate::live::hub::LiveHub, id: Uuid, obs: &[Obs]) -> Result<usize> {
    let rows: Vec<Observation> =
        obs.iter().map(|o| Observation { period_start: o.period_start, released_at: o.released_at, value: o.value }).collect();
    let changed = store::upsert_observations(pool, id, &rows).await?;
    let frequency = store::get_series(pool, id).await?.map(|s| s.frequency).unwrap_or_default();
    for o in changed.iter().rev().take(LIVE_POINTS).rev() {
        hub.publish_point(id, o.period_start, o.released_at, placed_at(o, true, &frequency), o.value);
    }
    Ok(changed.len())
}

/// The day after a period's last one, from its start and the series' frequency (`D W M Q S A`).
/// `None` for one that has no length: an irregular row, a frequency the provider left blank.
pub fn period_end(start: time::Date, frequency: &str) -> Option<time::Date> {
    let months = match frequency {
        "D" => return start.checked_add(time::Duration::days(1)),
        "W" => return start.checked_add(time::Duration::days(7)),
        "M" => 1,
        "Q" => 3,
        "S" => 6,
        "A" => 12,
        _ => return None,
    };
    let total = start.year() * 12 + i32::from(u8::from(start.month())) - 1 + months;
    let month = time::Month::try_from((total.rem_euclid(12) + 1) as u8).ok()?;
    let year = total.div_euclid(12);
    let day = start.day().min(month.length(year));
    time::Date::from_calendar_date(year, month, day).ok()
}

/// Where an observation sits on a time axis, in epoch ms. With `released`: when it was known,
/// its release if the source gives one, else the end of its period (a figure for January
/// cannot be read on January 1st). Without, or for a period with no length: its period start.
pub fn placed_at(o: &Observation, released: bool, frequency: &str) -> i64 {
    match (released, o.released_at) {
        (true, Some(t)) => (t.unix_timestamp_nanos() / 1_000_000) as i64,
        (true, None) => crate::fundamentals::date_ms(period_end(o.period_start, frequency).unwrap_or(o.period_start)),
        (false, _) => crate::fundamentals::date_ms(o.period_start),
    }
}

/// A series as `(epoch ms, value)` points, placed by [`placed_at`].
pub async fn points(pool: &PgPool, id: Uuid, released: bool, frequency: &str) -> Result<Vec<(i64, f64)>> {
    let obs = store::observations_released(pool, id, None, None).await?;
    Ok(obs.iter().map(|o| (placed_at(o, released, frequency), o.value)).collect())
}

/// Load the series every `series` node of these indicators reads, so the engine can lay them
/// onto the bars. An unknown id is an error naming it, never an all-empty input.
pub async fn bind_series(pool: &PgPool, defs: &mut crate::backtest::IndicatorDefs) -> Result<()> {
    for def in defs.values_mut() {
        bind_def(pool, def).await?;
    }
    Ok(())
}

/// [`bind_series`] for one indicator.
pub async fn bind_def(pool: &PgPool, def: &mut crate::backtest::custom::CustomIndicatorDef) -> Result<()> {
    for node in def.nodes.iter_mut() {
        if let crate::backtest::custom::Node::Series { series, at, points: slot, .. } = node {
            let id: Uuid = series.trim().parse().map_err(|_| anyhow!("series {series:?} is not a series id"))?;
            let Some(row) = store::get_series(pool, id).await? else {
                bail!("series {id} does not exist: pick another in the indicator");
            };
            *slot = Some(std::sync::Arc::new(points(pool, id, at.as_str() != "period", &row.frequency).await?));
        }
    }
    Ok(())
}

/// The series ids these indicators read.
pub fn series_read(defs: &crate::backtest::IndicatorDefs) -> Vec<Uuid> {
    defs.values()
        .flat_map(|d| d.nodes.iter())
        .filter_map(|n| match n {
            crate::backtest::custom::Node::Series { series, .. } => series.trim().parse().ok(),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn a_point_sits_at_its_release_when_known() {
        let o = Observation { period_start: date!(2024 - 01 - 01), released_at: Some(datetime!(2024-02-13 13:30 UTC)), value: 3.1 };
        assert_eq!(placed_at(&o, true, "M"), datetime!(2024-02-13 13:30 UTC).unix_timestamp() * 1000);
        assert_eq!(placed_at(&o, false, "M"), datetime!(2024-01-01 0:00 UTC).unix_timestamp() * 1000);
    }

    #[test]
    fn a_point_with_no_release_waits_for_its_period_to_end() {
        let bare = Observation { period_start: date!(2024 - 01 - 01), released_at: None, value: 3.1 };
        let ms = |t: time::OffsetDateTime| t.unix_timestamp() * 1000;
        assert_eq!(placed_at(&bare, true, "D"), ms(datetime!(2024-01-02 0:00 UTC)));
        assert_eq!(placed_at(&bare, true, "W"), ms(datetime!(2024-01-08 0:00 UTC)));
        assert_eq!(placed_at(&bare, true, "M"), ms(datetime!(2024-02-01 0:00 UTC)));
        assert_eq!(placed_at(&bare, true, "Q"), ms(datetime!(2024-04-01 0:00 UTC)));
        assert_eq!(placed_at(&bare, true, "A"), ms(datetime!(2025-01-01 0:00 UTC)));
        // Asked for the period, or a row with no length: where the source dated it.
        assert_eq!(placed_at(&bare, false, "M"), ms(datetime!(2024-01-01 0:00 UTC)));
        assert_eq!(placed_at(&bare, true, "I"), ms(datetime!(2024-01-01 0:00 UTC)));
        assert_eq!(placed_at(&bare, true, ""), ms(datetime!(2024-01-01 0:00 UTC)));
    }

    #[test]
    fn a_period_end_crosses_the_year_and_keeps_a_valid_day() {
        assert_eq!(period_end(date!(2024 - 12 - 01), "M"), Some(date!(2025 - 01 - 01)));
        assert_eq!(period_end(date!(2024 - 10 - 01), "Q"), Some(date!(2025 - 01 - 01)));
        assert_eq!(period_end(date!(2024 - 01 - 31), "M"), Some(date!(2024 - 02 - 29)));
        assert_eq!(period_end(date!(2024 - 07 - 01), "S"), Some(date!(2025 - 01 - 01)));
    }
}
