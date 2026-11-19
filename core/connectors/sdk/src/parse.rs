//! Parsers shared by every mapper: decimals from their JSON text, timestamps by format.
//!
//! A number is never routed through an `f64`. [`Dec`] reads the raw JSON token, so
//! `"0.1"` and `0.1` both land as the exact decimal 0.1.

use std::str::FromStr;

use otw_connector_model::{ConnectorError, Decimal, Money, Price, Qty, Timestamp};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

/// Parse a decimal written as `123.45`, `-1`, `1e-8` or `1.5E+3`.
pub fn dec(s: &str) -> Result<Decimal, ConnectorError> {
    let t = s.trim();
    if t.is_empty() {
        return Err(ConnectorError::decode("empty number"));
    }
    if t.contains(['e', 'E']) {
        return Decimal::from_scientific(t).map_err(|e| ConnectorError::decode(format!("number `{t}`: {e}")));
    }
    Decimal::from_str(t).map_err(|e| ConnectorError::decode(format!("number `{t}`: {e}")))
}

fn raw_text(raw: &RawValue) -> &str {
    let t = raw.get().trim();
    t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t)
}

/// A decimal field that may arrive as a JSON string or a JSON number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Dec(pub Decimal);

impl Dec {
    pub fn price(self) -> Price {
        Price(self.0)
    }
    pub fn qty(self) -> Qty {
        Qty(self.0)
    }
    pub fn money(self) -> Money {
        Money(self.0)
    }
}

impl<'de> Deserialize<'de> for Dec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(d)?;
        dec(raw_text(&raw)).map(Dec).map_err(|e| serde::de::Error::custom(e.message))
    }
}

/// Like [`Dec`], but `""` and `null` are `None` (venues use both for "not applicable").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OptDec(pub Option<Decimal>);

impl<'de> Deserialize<'de> for OptDec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Option::<Box<RawValue>>::deserialize(d)?;
        match raw {
            None => Ok(OptDec(None)),
            Some(r) => {
                let t = raw_text(&r);
                if t.is_empty() || t == "null" {
                    Ok(OptDec(None))
                } else {
                    dec(t).map(|v| OptDec(Some(v))).map_err(|e| serde::de::Error::custom(e.message))
                }
            }
        }
    }
}

/// An integer that may arrive as a JSON string or number (`"1700000000000"` or `1700000000000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Int(pub i64);

impl<'de> Deserialize<'de> for Int {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(d)?;
        let t = raw_text(&raw);
        t.parse::<i64>().map(Int).map_err(|_| serde::de::Error::custom(format!("integer `{t}`")))
    }
}

fn overflow(what: &str, v: i64) -> ConnectorError {
    ConnectorError::decode(format!("timestamp {v} {what} out of range"))
}

pub fn ts_secs(v: i64) -> Result<Timestamp, ConnectorError> {
    Timestamp::from_secs(v).ok_or_else(|| overflow("s", v))
}

pub fn ts_millis(v: i64) -> Result<Timestamp, ConnectorError> {
    Timestamp::from_millis(v).ok_or_else(|| overflow("ms", v))
}

pub fn ts_micros(v: i64) -> Result<Timestamp, ConnectorError> {
    Timestamp::from_micros(v).ok_or_else(|| overflow("us", v))
}

pub fn ts_nanos(v: i64) -> Timestamp {
    Timestamp::from_nanos(v)
}

/// Seconds with a decimal fraction, as text (`"1700000000.123456"`), exact to the nanosecond.
pub fn ts_secs_frac(s: &str) -> Result<Timestamp, ConnectorError> {
    let d = dec(s)?;
    let ns = d
        .checked_mul(Decimal::from(1_000_000_000_i64))
        .and_then(|x| i64::try_from(x.trunc()).ok())
        .ok_or_else(|| ConnectorError::decode(format!("timestamp `{s}` out of range")))?;
    Ok(Timestamp::from_nanos(ns))
}

/// RFC 3339 / ISO 8601 with an offset (`2024-01-02T03:04:05.123Z`).
pub fn ts_rfc3339(s: &str) -> Result<Timestamp, ConnectorError> {
    let t = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| ConnectorError::decode(format!("timestamp `{s}`: {e}")))?;
    Timestamp::from_offset_datetime(t).ok_or_else(|| ConnectorError::decode(format!("timestamp `{s}` out of range")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Row {
        a: Dec,
        b: Dec,
        c: OptDec,
        d: OptDec,
        e: Int,
    }

    #[test]
    fn decimals_keep_their_text_whether_string_or_number() {
        let r: Row = serde_json::from_str(r#"{"a":"0.1","b":0.30000000000000004,"c":"","d":null,"e":"17"}"#).unwrap();
        assert_eq!(r.a.0.to_string(), "0.1");
        assert_eq!(r.b.0.to_string(), "0.30000000000000004", "no f64 rounding on the way in");
        assert_eq!(r.c.0, None);
        assert_eq!(r.d.0, None);
        assert_eq!(r.e.0, 17);
    }

    #[test]
    fn scientific_and_garbage() {
        assert_eq!(dec("1e-8").unwrap().to_string(), "0.00000001");
        assert!(dec("").is_err());
        assert!(dec("abc").is_err());
        assert!(serde_json::from_str::<Dec>("true").is_err());
    }

    #[test]
    fn time_formats() {
        assert_eq!(ts_millis(1_700_000_000_123).unwrap().nanos(), 1_700_000_000_123_000_000);
        assert_eq!(ts_secs_frac("1700000000.123456789").unwrap().nanos(), 1_700_000_000_123_456_789);
        assert_eq!(ts_rfc3339("2023-11-14T22:13:20Z").unwrap(), ts_secs(1_700_000_000).unwrap());
        assert!(ts_millis(i64::MAX).is_err());
    }
}
