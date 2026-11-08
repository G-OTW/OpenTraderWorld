//! The `value` shape: one number per period instead of a candle.
//!
//! A custom source whose `shape` setting is `value` (CPI, a rate, a sentiment index, any time
//! series) is not a market: its rows are `(period_start, value)`, optionally with the instant
//! the value was released, and they are stored as a Fundamentals series (`fund_series` +
//! `fund_observations`), never as a histdata dataset. The same kinds carry it: `http` pulls
//! it, `push` receives it from a script, the file import reads it from a file.
//!
//! A row is a period: `period_start` is stored as the source wrote it (the date it starts),
//! never moved onto a calendar grid. `released_at` is what keeps a chart or a backtest from
//! reading a value before it was published. `frequency: irregular` means each row stands
//! alone (rate decisions, releases): nothing buckets it anywhere.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use super::http::{self, Col, TsUnit};

/// The fields a value row maps.
pub(crate) const VALUE_FIELDS: [&str; 3] = ["period_start", "value", "released_at"];

/// The frequencies a value source declares, as the settings spell them.
pub const FREQUENCIES: [&str; 6] = ["1d", "1W", "1M", "1Q", "1Y", "irregular"];

/// Whether a connector's settings ask for the value shape.
pub fn is_value_shape(cfg: &HashMap<String, String>) -> bool {
    http::get(cfg, "shape").is_some_and(|s| s.eq_ignore_ascii_case("value"))
}

/// The shape setting itself, checked: `ohlcv` (the default) or `value`.
pub(crate) fn check_shape(cfg: &HashMap<String, String>) -> Result<()> {
    match http::get(cfg, "shape").map(str::to_ascii_lowercase).as_deref() {
        None | Some("ohlcv") | Some("value") => Ok(()),
        Some(other) => bail!("shape {other:?}: use ohlcv (candles) or value (one number per period)"),
    }
}

/// A declared frequency as Fundamentals stores it (`D W M Q A`, `I` for irregular).
pub fn frequency_code(f: &str) -> Result<&'static str> {
    Ok(match f.trim() {
        "1d" => "D",
        "1W" | "1w" => "W",
        "1M" => "M",
        "1Q" => "Q",
        "1Y" | "1y" => "A",
        "irregular" => "I",
        other => bail!("frequency {other:?}: use {}", FREQUENCIES.join(", ")),
    })
}

/// One observation, as a value source gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obs {
    pub period_start: Date,
    pub released_at: Option<OffsetDateTime>,
    pub value: f64,
}

/// Whether a cell says there is no value for its period, as opposed to holding one that does
/// not read: a holiday in a daily series, a figure not published. Such a row is left out, never
/// stored as 0 or as the previous value; a chart and an indicator hold the last real one on read.
pub(crate) fn is_missing(raw: &str) -> bool {
    let s = raw.trim();
    s.is_empty() || matches!(s.to_ascii_lowercase().as_str(), "." | "-" | "na" | "n/a" | "#n/a" | "nan" | "null" | "none")
}

/// The tail of a report line naming the periods left out, empty when there are none.
pub fn missing_note(missing: usize) -> String {
    match missing {
        0 => String::new(),
        1 => ", 1 period without a value skipped".to_string(),
        n => format!(", {n} periods without a value skipped"),
    }
}

/// What a rows value held.
#[derive(Debug, Default)]
pub struct ValueRows {
    /// Sorted by period, one per period.
    pub obs: Vec<Obs>,
    /// The periods the source left without a value, none of which is in `obs`.
    pub missing: BTreeSet<Date>,
}

/// How one row becomes one observation.
#[derive(Debug, Clone)]
pub struct ValueMapping {
    pub columns: Vec<(&'static str, Col)>,
    pub ts_unit: TsUnit,
    pub unit: String,
    /// Fundamentals' code (`D W M Q A I`).
    pub frequency: &'static str,
}

impl ValueMapping {
    /// The value settings (`columns` with `period_start` and `value`, optional `released_at`;
    /// `ts_unit` for numeric stamps; `frequency`; optional `unit`), read and checked.
    pub fn parse(cfg: &HashMap<String, String>) -> Result<ValueMapping> {
        let raw = http::get(cfg, "columns").ok_or_else(|| anyhow!("Columns is required"))?;
        let obj = match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(m)) => m,
            Ok(_) => bail!("columns must be a JSON object"),
            Err(e) => bail!("columns is not valid JSON: {e}"),
        };
        let mut columns: Vec<(&'static str, Col)> = Vec::new();
        for (field, col) in obj {
            let Some(f) = VALUE_FIELDS.iter().copied().find(|f| *f == field) else {
                bail!("columns: {field:?} is not a value field. Use {}", VALUE_FIELDS.join(", "));
            };
            let col = match col {
                Value::Number(n) => Col::Index(
                    n.as_u64()
                        .and_then(|i| usize::try_from(i).ok())
                        .ok_or_else(|| anyhow!("columns: {field} must be a position (0, 1, ...) or a key"))?,
                ),
                Value::String(s) if !s.trim().is_empty() => Col::Key(s.trim().to_string()),
                _ => bail!("columns: {field} must be a position (0, 1, ...) or a key"),
            };
            columns.push((f, col));
        }
        for required in ["period_start", "value"] {
            if !columns.iter().any(|(f, _)| *f == required) {
                bail!("columns: {required} is required for a value source");
            }
        }
        columns.sort_by_key(|(f, _)| VALUE_FIELDS.iter().position(|x| x == f));
        let ts_unit = match http::get(cfg, "ts_unit") {
            None | Some("iso") => TsUnit::Iso,
            Some("s") => TsUnit::Secs,
            Some("ms") => TsUnit::Millis,
            Some(other) => bail!("timestamp unit {other:?}: use s, ms or iso"),
        };
        let frequency = frequency_code(
            http::get(cfg, "frequency").ok_or_else(|| anyhow!("Frequency is required for a value source: {}", FREQUENCIES.join(", ")))?,
        )?;
        Ok(ValueMapping { columns, ts_unit, unit: http::get(cfg, "unit").unwrap_or("").to_string(), frequency })
    }

    /// Every observation of a rows value (a list of lists, a list of objects, or one object
    /// of parallel lists), sorted by period, one per period (the later row wins: a revision).
    /// A row that does not read fails the whole set, naming it, so a wrong mapping is never a
    /// thinner series; a row whose value [`is_missing`] is left out and counted.
    pub fn read(&self, rows: &Value) -> Result<ValueRows> {
        let grid = http::to_grid(rows, &self.columns)?;
        let idx = |f: &str| grid.headers.iter().position(|h| h == f);
        let (Some(pi), Some(vi)) = (idx("period_start"), idx("value")) else {
            bail!("columns: period_start and value are required");
        };
        let ri = idx("released_at");
        let mut by_period: BTreeMap<Date, Obs> = BTreeMap::new();
        let mut missing = BTreeSet::new();
        for (n, row) in grid.rows.iter().enumerate() {
            let at = |i: usize| row.get(i).map(String::as_str).unwrap_or("").trim();
            let period_start = read_date(at(pi), self.ts_unit)
                .map_err(|e| anyhow!("row {}: period_start {:?}: {e}", n + 1, at(pi)))?;
            let raw = at(vi);
            if is_missing(raw) {
                missing.insert(period_start);
                continue;
            }
            let value: f64 = raw
                .parse()
                .ok()
                .filter(|v: &f64| v.is_finite())
                .ok_or_else(|| anyhow!("row {}: value {raw:?} is not a number", n + 1))?;
            let released_at = match ri.map(at).filter(|s| !s.is_empty()) {
                None => None,
                Some(s) => Some(read_instant(s, self.ts_unit).map_err(|e| anyhow!("row {}: released_at {s:?}: {e}", n + 1))?),
            };
            by_period.insert(period_start, Obs { period_start, released_at, value });
        }
        missing.retain(|d| !by_period.contains_key(d));
        Ok(ValueRows { obs: by_period.into_values().collect(), missing })
    }

    /// The observations of a rows value, see [`ValueMapping::read`].
    pub fn observations(&self, rows: &Value) -> Result<Vec<Obs>> {
        Ok(self.read(rows)?.obs)
    }
}

/// An instant: a number in the unit, else RFC 3339, a naive `YYYY-MM-DD HH:MM[:SS]` read as
/// UTC, or a date (its midnight UTC).
pub(crate) fn read_instant(s: &str, unit: TsUnit) -> Result<OffsetDateTime> {
    let s = s.trim();
    if let Ok(n) = s.parse::<f64>() {
        let nanos = match unit {
            TsUnit::Millis => n * 1e6,
            // An ISO source quoting an epoch is read in seconds.
            TsUnit::Secs | TsUnit::Iso => n * 1e9,
        };
        return OffsetDateTime::from_unix_timestamp_nanos(nanos as i128).map_err(|e| anyhow!("{e}"));
    }
    if let Ok(t) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(t.to_offset(time::UtcOffset::UTC));
    }
    let naive = s.replace(' ', "T");
    for candidate in [format!("{naive}Z"), format!("{naive}:00Z")] {
        if let Ok(t) = OffsetDateTime::parse(&candidate, &Rfc3339) {
            return Ok(t);
        }
    }
    let d = read_date_text(s)?;
    Ok(OffsetDateTime::new_utc(d, time::Time::MIDNIGHT))
}

/// A period start: a number in the unit (its UTC date), an instant's UTC date, or a date
/// written `YYYY-MM-DD`, `YYYY-MM`, `YYYY`, `YYYY-Qn`.
pub(crate) fn read_date(s: &str, unit: TsUnit) -> Result<Date> {
    let s = s.trim();
    if s.is_empty() {
        bail!("empty");
    }
    if let Ok(d) = read_date_text(s) {
        return Ok(d);
    }
    Ok(read_instant(s, unit)?.date())
}

fn read_date_text(s: &str) -> Result<Date> {
    let month = |m: u8| time::Month::try_from(m).map_err(|e| anyhow!("{e}"));
    let num = |p: &str| p.parse::<i32>().map_err(|_| anyhow!("not a date"));
    let parts: Vec<&str> = s.split(['-', '/']).collect();
    match parts.as_slice() {
        [y, q] if q.len() == 2 && (q.starts_with('Q') || q.starts_with('q')) => {
            let n: u8 = q[1..].parse().map_err(|_| anyhow!("not a quarter"))?;
            if !(1..=4).contains(&n) {
                bail!("quarter {n} is not 1 to 4");
            }
            Ok(Date::from_calendar_date(num(y)?, month((n - 1) * 3 + 1)?, 1)?)
        }
        [y] if y.len() == 4 => Ok(Date::from_calendar_date(num(y)?, time::Month::January, 1)?),
        [y, m] if y.len() == 4 => Ok(Date::from_calendar_date(num(y)?, month(num(m)? as u8)?, 1)?),
        [y, m, d] if y.len() == 4 && d.len() <= 2 => {
            Ok(Date::from_calendar_date(num(y)?, month(num(m)? as u8)?, num(d)? as u8)?)
        }
        _ => bail!("not a date (YYYY-MM-DD, YYYY-MM, YYYY or YYYY-Qn)"),
    }
}

/// The posted `observations` of a push payload, read with the mapping.
pub(crate) fn parse_push(m: &ValueMapping, body: &Value) -> Result<(String, Option<String>, ValueRows)> {
    let obj = body.as_object().ok_or_else(|| anyhow!("the body must be a JSON object with series and observations"))?;
    let series = obj
        .get("series")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("series is required: the code of the series these observations belong to"))?
        .to_string();
    let title = obj.get("title").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let rows = obj.get("observations").ok_or_else(|| anyhow!("observations is required (this is a value source, not bars)"))?;
    if rows.as_array().is_some_and(|a| a.len() > 10_000) {
        bail!("at most 10000 observations per payload: send history in batches");
    }
    let read = m.read(rows)?;
    if read.obs.is_empty() {
        bail!("observations is empty: no row carries a value");
    }
    Ok((series, title, read))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::macros::{date, datetime};

    fn cfg(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn a_value_mapping_reads_periods_values_and_releases() {
        let m = ValueMapping::parse(&cfg(&[
            ("shape", "value"),
            ("columns", r#"{"period_start": "date", "value": "v", "released_at": "rel"}"#),
            ("frequency", "1M"),
            ("unit", "%"),
        ]))
        .unwrap();
        assert_eq!((m.frequency, m.unit.as_str()), ("M", "%"));
        let rows = json!([
            {"date": "2024-02", "v": "3.2", "rel": "2024-03-12T12:30:00Z"},
            {"date": "2024-01-01", "v": 3.1, "rel": "2024-02-13 13:30"},
            {"date": "2024-02-01", "v": 3.3},
        ]);
        let obs = m.observations(&rows).unwrap();
        assert_eq!(obs.len(), 2, "one row per period, the later reading wins");
        assert_eq!(obs[0], Obs { period_start: date!(2024 - 01 - 01), released_at: Some(datetime!(2024-02-13 13:30 UTC)), value: 3.1 });
        assert_eq!(obs[1].period_start, date!(2024 - 02 - 01));
        assert_eq!((obs[1].value, obs[1].released_at), (3.3, None));
    }

    #[test]
    fn a_value_row_that_does_not_read_is_named() {
        let m = ValueMapping::parse(&cfg(&[("columns", r#"{"period_start": 0, "value": 1}"#), ("frequency", "irregular")])).unwrap();
        assert_eq!(m.frequency, "I");
        let err = m.observations(&json!([["2024-01-01", "soon"]])).unwrap_err().to_string();
        assert!(err.contains("row 1") && err.contains("not a number"), "{err}");
        let err = m.observations(&json!([["soon", 1]])).unwrap_err().to_string();
        assert!(err.contains("period_start"), "{err}");
        // Quarters and epochs read as their period's start date.
        let obs = m.observations(&json!([["2024-Q2", 1], [1704067200, 2]])).unwrap();
        assert_eq!(obs[0].period_start, date!(2024 - 01 - 01));
        assert_eq!(obs[1].period_start, date!(2024 - 04 - 01));
    }

    #[test]
    fn a_period_without_a_value_is_left_out_and_counted() {
        let m = ValueMapping::parse(&cfg(&[("columns", r#"{"period_start": 0, "value": 1}"#), ("frequency", "1d")])).unwrap();
        let rows = json!([
            ["2024-01-01", 1.5],
            ["2024-01-02", "NA"],
            ["2024-01-03", null],
            ["2024-01-04", "."],
            ["2024-01-05", ""],
            ["2024-01-06", "n/a"],
            ["2024-01-07", 2.5],
        ]);
        let read = m.read(&rows).unwrap();
        assert_eq!(read.obs.iter().map(|o| o.value).collect::<Vec<_>>(), vec![1.5, 2.5], "nothing is filled in");
        assert_eq!(read.missing.len(), 5);

        // A period with a value is not missing, whichever row came first.
        let read = m.read(&json!([["2024-01-01", "NA"], ["2024-01-01", 1.5], ["2024-01-02", 2], ["2024-01-02", "NA"]])).unwrap();
        assert_eq!(read.obs.len(), 2);
        assert!(read.missing.is_empty());

        // A broken date still fails, even on a row with no value: that is a wrong mapping.
        assert!(m.read(&json!([["soon", "NA"]])).unwrap_err().to_string().contains("period_start"));
    }

    #[test]
    fn value_settings_need_a_frequency_and_the_two_columns() {
        assert!(ValueMapping::parse(&cfg(&[("columns", r#"{"period_start": 0, "value": 1}"#)])).is_err());
        assert!(ValueMapping::parse(&cfg(&[("columns", r#"{"value": 1}"#), ("frequency", "1M")])).is_err());
        assert!(ValueMapping::parse(&cfg(&[("columns", r#"{"period_start": 0, "close": 1}"#), ("frequency", "1M")])).is_err());
        assert!(ValueMapping::parse(&cfg(&[("columns", r#"{"period_start": 0, "value": 1}"#), ("frequency", "2M")])).is_err());
        assert!(check_shape(&cfg(&[("shape", "bars")])).is_err());
        assert!(is_value_shape(&cfg(&[("shape", "Value")])));
    }

    #[test]
    fn a_pushed_value_payload_names_its_series() {
        let m = ValueMapping::parse(&cfg(&[("columns", r#"{"period_start": "p", "value": "v"}"#), ("frequency", "1W")])).unwrap();
        let (series, title, read) =
            parse_push(&m, &json!({"series": "SENT", "title": "Sentiment", "observations": [{"p": "2024-01-08", "v": 55}, {"p": "2024-01-15", "v": "NA"}]})).unwrap();
        assert_eq!((series.as_str(), title.as_deref(), read.obs.len(), read.missing.len()), ("SENT", Some("Sentiment"), 1, 1));
        assert!(parse_push(&m, &json!({"series": "S", "observations": [{"p": "2024-01-08", "v": "NA"}]})).unwrap_err().to_string().contains("no row carries a value"));
        assert!(parse_push(&m, &json!({"observations": []})).unwrap_err().to_string().contains("series"));
        assert!(parse_push(&m, &json!({"series": "S", "bars": []})).unwrap_err().to_string().contains("observations"));
    }
}
