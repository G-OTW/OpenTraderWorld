//! Deribit refusals. A JSON-RPC error answers HTTP 400 (measured, a bad key included) with
//! `{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid params","data":{"reason":
//! "instrument not found","param":"instrument_name"}}}`; a success carries `result`. The
//! code decides, the reason is kept in the message. Table mirrored in PROVIDER.md.

use otw_connector_model::{ConnectorError, ErrorKind};
use otw_connector_sdk::{http, RawFrame};
use serde::Deserialize;

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// What a code means, given the parameter an `Invalid params` names.
pub fn kind_of(code: i64, param: Option<&str>) -> ErrorKind {
    match code {
        // too_many_requests, too_many_concurrent_requests, matching_engine_queue_full.
        10028 | 10066 | 10047 => ErrorKind::RateLimited,
        // authorization_required, invalid_credentials (a bad signature or a stale timestamp
        // among them), user_not_found, unauthorized (a bad or expired token),
        // login_over_limit, security_key_authorization_over_limit.
        10000 | 13004 | 13007 | 13009 | 12003 | 12998 => ErrorKind::Auth,
        // forbidden, permission_denied, scope_exceeded, unavailable_in_subaccount,
        // verification_required, account_blocked, country_is_banned, account_locked.
        13021 | 11042 | 13403 | 13012 | 13031 | 13002 | 12005 | 11098 => ErrorKind::Permission,
        // invalid_or_unsupported_instrument, not_found, orderbook_closed.
        10020 | 13020 | 13019 => ErrorKind::NotFound,
        // `Invalid params` on the instrument: "instrument not found" or "wrong format"
        // (measured).
        -32602 if param == Some("instrument_name") => ErrorKind::NotFound,
        // not_supported_for_coinbase_routed_spot (also spelt not_supported_for_csr_spot).
        11060 => ErrorKind::Unsupported,
        // system_maintenance.
        11051 => ErrorKind::ProviderDown,
        // error (general failure), retry, settlement_in_progress, internal_server_error,
        // method_switched_off_by_admin, temporarily_unavailable, unavailable, timed_out,
        // unauthenticated_public_requests_temporarily_disabled.
        10001 | 10040 | 10041 | 11094 | 13025 | 13028 | 13503 | 13888 | 11056 => ErrorKind::Transient,
        // Invalid params, Method not found, Parse error, Missing params, bad arguments...
        _ => ErrorKind::InvalidRequest,
    }
}

pub fn from_frame(frame: &RawFrame) -> Option<ConnectorError> {
    if let Some(e) = http::timeout(frame, "deribit") {
        return Some(e);
    }
    let RawFrame::Http { body, endpoint_id, status, .. } = frame else { return None };
    match serde_json::from_slice::<Answer>(body) {
        Ok(Answer { error: Some(err) }) => {
            let data = err.data.as_ref();
            let param = data.and_then(|d| d.get("param")).and_then(|p| p.as_str());
            let reason = data.and_then(|d| d.get("reason").or_else(|| d.get("invalid"))).and_then(|r| r.as_str());
            let kind = if *status == 429 { ErrorKind::RateLimited } else { kind_of(err.code, param) };
            let detail = match (reason, param) {
                (Some(r), Some(p)) => format!(" ({p}: {r})"),
                (Some(r), None) => format!(" ({r})"),
                _ => String::new(),
            };
            let msg = Some(err.message.trim()).filter(|m| !m.is_empty()).unwrap_or("refused");
            let mut e = ConnectorError::new(kind, format!("deribit {endpoint_id}: {msg}{detail} ({})", err.code)).with_code(err.code.to_string());
            e.retry_after_ms = http::retry_after_ms(frame);
            Some(e)
        }
        // A body with no `error` is an answer, whatever the status says it is.
        Ok(Answer { error: None }) if (200..300).contains(status) => None,
        _ => http::classify(frame, "deribit"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use otw_connector_testkit::http_frame;

    #[test]
    fn the_code_decides() {
        assert!(from_frame(&http_frame(1, "x", 200, r#"{"jsonrpc":"2.0","result":1792403985955,"usIn":1,"usOut":2,"usDiff":1,"testnet":false}"#)).is_none());
        // Recorded answers, 2026-10-19.
        let e = from_frame(&http_frame(1, "chart", 400, r#"{"usOut":1792403985799342,"usIn":1792403985799330,"usDiff":12,"testnet":false,"jsonrpc":"2.0","error":{"message":"Invalid params","data":{"reason":"instrument not found","param":"instrument_name"},"code":-32602}}"#)).unwrap();
        assert_eq!((e.kind, e.provider_code.as_deref()), (ErrorKind::NotFound, Some("-32602")));
        assert!(e.message.contains("instrument not found"), "{}", e.message);
        let e = from_frame(&http_frame(1, "chart", 400, r#"{"jsonrpc":"2.0","error":{"message":"Invalid params","data":{"reason":"unsupported resolution","param":"resolution"},"code":-32602}}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::InvalidRequest, "a bad resolution is our bug, not a missing instrument");
        let e = from_frame(&http_frame(1, "positions", 400, r#"{"jsonrpc":"2.0","error":{"code":13004,"message":"invalid_credentials"},"testnet":false,"usIn":1792403744470882,"usOut":1792403744471422,"usDiff":540}"#)).unwrap();
        assert_eq!(e.kind, ErrorKind::Auth);
        let e = from_frame(&http_frame(1, "positions", 400, r#"{"jsonrpc":"2.0","error":{"code":13004,"data":{"invalid":"timestamp"},"message":"invalid_credentials"}}"#)).unwrap();
        assert!(e.kind == ErrorKind::Auth && e.message.contains("timestamp"), "{}", e.message);
        assert_eq!(from_frame(&http_frame(1, "positions", 400, r#"{"jsonrpc":"2.0","error":{"code":13009,"data":{"reason":"invalid_token"},"message":"unauthorized"}}"#)).unwrap().kind, ErrorKind::Auth);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"jsonrpc":"2.0","error":{"code":10028,"message":"too_many_requests"}}"#)).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"jsonrpc":"2.0","error":{"code":13021,"message":"forbidden"}}"#)).unwrap().kind, ErrorKind::Permission);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"jsonrpc":"2.0","error":{"code":11060,"message":"not_supported_for_coinbase_routed_spot"}}"#)).unwrap().kind, ErrorKind::Unsupported);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"jsonrpc":"2.0","error":{"code":10041,"message":"settlement_in_progress"}}"#)).unwrap().kind, ErrorKind::Transient);
        assert_eq!(from_frame(&http_frame(1, "x", 400, r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"}}"#)).unwrap().kind, ErrorKind::InvalidRequest);
        // The 33rd connection from one address is refused by status.
        assert_eq!(from_frame(&http_frame(1, "x", 429, "Too Many Requests")).unwrap().kind, ErrorKind::RateLimited);
        assert_eq!(from_frame(&http_frame(1, "x", 502, "<html>bad gateway</html>")).unwrap().kind, ErrorKind::Transient);
    }
}
