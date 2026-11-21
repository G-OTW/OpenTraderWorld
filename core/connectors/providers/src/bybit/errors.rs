//! Bybit answers with `retCode` in the envelope (`0` = success), usually with HTTP 200 for a
//! refusal. An IP over its limit gets HTTP 403 instead; so does a refused country, from the CDN.
//! Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "retCode", default)]
    ret_code: Option<i64>,
    #[serde(rename = "retMsg", default)]
    ret_msg: Option<String>,
}

/// How long Bybit bans an IP over its limit (rate-limit page: "at least 10 minutes").
const IP_BAN_MS: u64 = 600_000;

fn kind_of(code: i64, msg: &str) -> ErrorKind {
    match code {
        // Too many visits, IP over its limit, IP banned for a while.
        10006 | 10018 | 10009 => ErrorKind::RateLimited,
        // Timestamp outside the window, key invalid, signature, auth failed, IP not on the
        // key's list, key expired.
        10002 | 10003 | 10004 | 10007 | 10010 | 33004 => ErrorKind::Auth,
        // The key lacks the permission; the region is refused by compliance.
        10005 | 10024 => ErrorKind::Permission,
        // Spot: invalid symbol.
        170121 => ErrorKind::NotFound,
        // A parameter error that names the symbol: the venue does not list it.
        10001 if msg.to_ascii_lowercase().contains("symbol") => ErrorKind::NotFound,
        10000 | 10016 => ErrorKind::Transient,
        _ => ErrorKind::InvalidRequest,
    }
}

pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "bybit") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    if *status == 403 {
        let text = String::from_utf8_lossy(body);
        let snippet: String = text.chars().take(160).collect();
        // Bybit documents a 403 as the IP ban ("access too frequent", ten minutes). A refused
        // country gets a 403 too, but from the CDN, as its own HTML page.
        let lower = text.to_ascii_lowercase();
        let region = lower.contains("cloudfront") || lower.contains("your country") || lower.contains("could not be satisfied");
        let mut e = if region {
            ConnectorError::new(ErrorKind::Permission, format!("bybit {endpoint_id}: refused from this country (HTTP 403): {snippet}"))
        } else {
            let mut e = ConnectorError::new(ErrorKind::RateLimited, format!("bybit {endpoint_id}: this IP is over Bybit's limit and banned for ten minutes (HTTP 403): {snippet}"));
            e.retry_after_ms = Some(http::retry_after_ms(frame).unwrap_or(IP_BAN_MS));
            e
        };
        e = e.with_code("403".to_string());
        return Some(e);
    }
    let env = serde_json::from_slice::<Envelope>(body).ok();
    match env.as_ref().and_then(|e| e.ret_code) {
        Some(0) => None,
        Some(code) => {
            let msg = env.as_ref().and_then(|e| e.ret_msg.clone()).unwrap_or_default();
            let kind = if *status == 429 { ErrorKind::RateLimited } else { kind_of(code, &msg) };
            let message = if kind == ErrorKind::NotFound {
                format!("bybit {endpoint_id}: bad symbol ({code} {msg})")
            } else {
                format!("bybit {endpoint_id}: {msg} (code {code})")
            };
            let mut e = ConnectorError::new(kind, message).with_code(code.to_string());
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        None => http::classify(frame, "bybit").or_else(|| Some(ConnectorError::decode(format!("bybit {endpoint_id}: no retCode")))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_ret_code_decides() {
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"retCode":0,"retMsg":"OK","result":{}}"#)).is_none());
        // Recorded from the live venue: a kline for a symbol it does not list.
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"retCode":10001,"retMsg":"Not supported symbols","result":{}}"#)).unwrap().kind, ErrorKind::NotFound);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"retCode":10001,"retMsg":"params error: limit"}"#)).unwrap().kind, ErrorKind::InvalidRequest);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"retCode":10003,"retMsg":"API key is invalid."}"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 200, r#"{"retCode":10006,"retMsg":"Too many visits!"}"#)).unwrap().kind, ErrorKind::RateLimited);
        let ban = from_frame(&http_frame(1, "x", 403, "access too frequent")).unwrap();
        assert_eq!((ban.kind, ban.retry_after_ms), (ErrorKind::RateLimited, Some(600_000)));
        // An empty body is the ban too: that is what Bybit documents a 403 to mean.
        assert_eq!(from_frame(&http_frame(1, "x", 403, "")).unwrap().kind, ErrorKind::RateLimited);
        let region = "<HTML><HEAD><TITLE>ERROR: The request could not be satisfied</TITLE></HEAD><BODY>The Amazon CloudFront distribution is configured to block access from your country.</BODY></HTML>";
        assert_eq!(from_frame(&http_frame(1, "x", 403, region)).unwrap().kind, ErrorKind::Permission);
        assert_eq!(from_frame(&http_frame(1, "x", 503, "busy")).unwrap().kind, ErrorKind::Transient);
    }
}
