//! MessagePack on the wire, JSON to the feed.
//!
//! One venue socket speaks MessagePack only (Alpaca options: a JSON connection is refused
//! with 412). Rather than teach an adapter a second encoding, the transport transcodes: the
//! frames the adapter writes as JSON go out as MessagePack, and what comes back is handed to
//! it as JSON. The one type JSON lacks is MessagePack's timestamp (extension -1), which
//! becomes the RFC 3339 string with nanoseconds the JSON socket sends in the same place.

use anyhow::{anyhow, Result};
use rmpv::Value as Mp;
use serde_json::Value as Js;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// A JSON frame as MessagePack bytes.
pub fn from_json(text: &str) -> Result<Vec<u8>> {
    let v: Js = serde_json::from_str(text).map_err(|e| anyhow!("frame is not JSON: {e}"))?;
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &to_mp(v)).map_err(|e| anyhow!("msgpack encode: {e}"))?;
    Ok(out)
}

/// MessagePack bytes as one JSON text.
pub fn to_json(bytes: &[u8]) -> Result<String> {
    let mut rd = bytes;
    let v = rmpv::decode::read_value(&mut rd).map_err(|e| anyhow!("msgpack decode: {e}"))?;
    Ok(to_js(v)?.to_string())
}

fn to_mp(v: Js) -> Mp {
    match v {
        Js::Null => Mp::Nil,
        Js::Bool(b) => Mp::Boolean(b),
        Js::Number(n) => match (n.as_u64(), n.as_i64(), n.as_f64()) {
            (Some(u), _, _) => Mp::from(u),
            (_, Some(i), _) => Mp::from(i),
            (_, _, Some(f)) => Mp::F64(f),
            _ => Mp::Nil,
        },
        Js::String(s) => Mp::from(s),
        Js::Array(a) => Mp::Array(a.into_iter().map(to_mp).collect()),
        Js::Object(o) => Mp::Map(o.into_iter().map(|(k, v)| (Mp::from(k), to_mp(v))).collect()),
    }
}

fn to_js(v: Mp) -> Result<Js> {
    Ok(match v {
        Mp::Nil => Js::Null,
        Mp::Boolean(b) => Js::Bool(b),
        Mp::Integer(i) => match (i.as_u64(), i.as_i64()) {
            (Some(u), _) => Js::from(u),
            (_, Some(i)) => Js::from(i),
            _ => Js::Null,
        },
        // A non-finite float has no JSON form: null, which every reader treats as absent.
        Mp::F32(f) => serde_json::Number::from_f64(f64::from(f)).map_or(Js::Null, Js::Number),
        Mp::F64(f) => serde_json::Number::from_f64(f).map_or(Js::Null, Js::Number),
        Mp::String(s) => Js::String(s.into_str().unwrap_or_default()),
        Mp::Binary(_) => Js::Null,
        Mp::Array(a) => Js::Array(a.into_iter().map(to_js).collect::<Result<_>>()?),
        Mp::Map(m) => {
            let mut o = serde_json::Map::with_capacity(m.len());
            for (k, v) in m {
                let key = match k {
                    Mp::String(s) => s.into_str().unwrap_or_default(),
                    other => other.to_string(),
                };
                o.insert(key, to_js(v)?);
            }
            Js::Object(o)
        }
        Mp::Ext(-1, data) => Js::String(timestamp(&data)?),
        Mp::Ext(..) => Js::Null,
    })
}

/// MessagePack's timestamp extension (32, 64 or 96 bits) as RFC 3339.
fn timestamp(d: &[u8]) -> Result<String> {
    let (secs, nanos): (i64, u32) = match d.len() {
        4 => (i64::from(u32::from_be_bytes(d.try_into()?)), 0),
        8 => {
            let v = u64::from_be_bytes(d.try_into()?);
            ((v & 0x3_ffff_ffff) as i64, (v >> 34) as u32)
        }
        12 => (i64::from_be_bytes(d[4..].try_into()?), u32::from_be_bytes(d[..4].try_into()?)),
        n => return Err(anyhow!("msgpack timestamp of {n} bytes")),
    };
    let t = OffsetDateTime::from_unix_timestamp_nanos(i128::from(secs) * 1_000_000_000 + i128::from(nanos)).map_err(|e| anyhow!("msgpack timestamp: {e}"))?;
    t.format(&Rfc3339).map_err(|e| anyhow!("msgpack timestamp: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 64-bit form: nanoseconds in the high 30 bits, seconds in the low 34.
    fn ts64(secs: u64, nanos: u64) -> Mp {
        Mp::Ext(-1, ((nanos << 34) | secs).to_be_bytes().to_vec())
    }

    #[test]
    fn a_quote_reads_as_the_json_socket_writes_it() {
        // Alpaca's option quote, as MessagePack: `t` is a timestamp extension.
        let q = Mp::Array(vec![Mp::Map(vec![
            (Mp::from("T"), Mp::from("q")),
            (Mp::from("S"), Mp::from("SPXW240327P04925000")),
            (Mp::from("t"), ts64(1_710_244_778, 897_261_568)),
            (Mp::from("bp"), Mp::F64(9.46)),
            (Mp::from("bs"), Mp::from(53u64)),
            (Mp::from("ap"), Mp::F64(9.66)),
            (Mp::from("as"), Mp::from(38u64)),
            (Mp::from("c"), Mp::from("A")),
        ])]);
        let mut bytes = Vec::new();
        rmpv::encode::write_value(&mut bytes, &q).unwrap();
        let js: Js = serde_json::from_str(&to_json(&bytes).unwrap()).unwrap();
        assert_eq!(
            js,
            serde_json::json!([{"T":"q","S":"SPXW240327P04925000","t":"2024-03-12T11:59:38.897261568Z","bp":9.46,"bs":53,"ap":9.66,"as":38,"c":"A"}])
        );
    }

    #[test]
    fn every_timestamp_width_reads() {
        assert_eq!(timestamp(&1_700_000_000u32.to_be_bytes()).unwrap(), "2023-11-14T22:13:20Z");
        let mut d96 = 5u32.to_be_bytes().to_vec();
        d96.extend((-1i64).to_be_bytes());
        assert_eq!(timestamp(&d96).unwrap(), "1969-12-31T23:59:59.000000005Z");
        assert!(timestamp(&[1, 2, 3]).is_err());
    }

    #[test]
    fn a_frame_written_as_json_goes_out_as_the_same_map() {
        let text = r#"{"action":"subscribe","quotes":["SPY251219C00650000"],"n":-3,"f":1.5}"#;
        let bytes = from_json(text).unwrap();
        let back: Js = serde_json::from_str(&to_json(&bytes).unwrap()).unwrap();
        assert_eq!(back, serde_json::from_str::<Js>(text).unwrap());
        assert!(from_json("not json").is_err());
        assert!(to_json(&[0x81]).is_err(), "a map cut before its entry");
    }
}
