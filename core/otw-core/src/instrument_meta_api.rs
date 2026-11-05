//! Per-instrument facts scripts read through `syminfo.*` (tick size, exchange timezone, …).
//!
//! - `GET /api/histdata/instrument-meta?provider=&asset_type=&ticker=` → `{meta|null, filled, source?}`
//!   (a future's empty tick size, point value and currency come from `futures_specs`)
//! - `PUT /api/histdata/instrument-meta` (whole record; a null field = unknown)

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use otw_store::instrument_meta::{self as store, InstrumentMeta};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/histdata/instrument-meta", get(get_meta).put(put_meta))
}

#[derive(Deserialize)]
struct Coords {
    provider: String,
    asset_type: String,
    ticker: String,
}

/// What a script, a chart and the paper broker read for an instrument: the stored facts, with
/// what a future leaves empty filled from its contract specification (`futures_specs`). An
/// error is a future whose root the specification cannot settle (listed on several markets).
pub(crate) async fn resolved(pool: &sqlx::PgPool, provider: &str, asset_type: &str, ticker: &str) -> anyhow::Result<Option<InstrumentMeta>> {
    let m = store::get(pool, provider, asset_type, ticker).await?;
    crate::futures_specs::complete(provider, asset_type, ticker, m)
}

/// [`resolved`] for a reader that runs without the specification rather than stop on it (a
/// backtest, an alert): an unsettled root reads as the stored facts alone.
pub(crate) async fn resolved_or_stored(pool: &sqlx::PgPool, provider: &str, asset_type: &str, ticker: &str) -> anyhow::Result<Option<InstrumentMeta>> {
    let m = store::get(pool, provider, asset_type, ticker).await?;
    Ok(crate::futures_specs::complete(provider, asset_type, ticker, m.clone()).unwrap_or(m))
}

/// The facts as they will be read. `filled` names the fields taken from the futures
/// specification rather than stored, and `spec_error` says why a future's could not be.
async fn get_meta(State(state): State<AppState>, Query(c): Query<Coords>) -> Result<Json<Value>, ApiError> {
    let stored = store::get(&state.pool, &c.provider, &c.asset_type, &c.ticker).await?;
    match crate::futures_specs::complete(&c.provider, &c.asset_type, &c.ticker, stored.clone()) {
        Ok(m) => {
            let was = |f: fn(&InstrumentMeta) -> bool| stored.as_ref().is_some_and(f);
            let mut filled = Vec::new();
            if let Some(m) = &m {
                if m.mintick.is_some() && !was(|s| s.mintick.is_some()) {
                    filled.push("mintick");
                }
                if m.pointvalue.is_some() && !was(|s| s.pointvalue.is_some()) {
                    filled.push("pointvalue");
                }
                if m.currency.is_some() && !was(|s| s.currency.is_some()) {
                    filled.push("currency");
                }
            }
            let source = (!filled.is_empty()).then_some(crate::futures_specs::SOURCE);
            Ok(Json(json!({ "meta": m, "filled": filled, "source": source })))
        }
        Err(e) => Ok(Json(json!({ "meta": stored, "filled": [], "spec_error": format!("{e:#}") }))),
    }
}

async fn put_meta(State(state): State<AppState>, Json(m): Json<InstrumentMeta>) -> Result<Json<Value>, ApiError> {
    validate_instrument_meta(&m)?;
    store::put(&state.pool, &m).await?;
    Ok(Json(json!({ "ok": true })))
}

/// The same rules the script engine applies when it reads the field: a value it could not
/// use is refused here, so it never fails later inside a script.
pub(crate) fn validate_instrument_meta(m: &InstrumentMeta) -> Result<(), ApiError> {
    if [&m.provider, &m.asset_type, &m.ticker].iter().any(|s| s.trim().is_empty()) {
        return Err(ApiError::bad_request("provider, asset_type and ticker are required"));
    }
    for (v, what) in [(m.mintick, "mintick"), (m.pointvalue, "pointvalue"), (m.margin, "margin")] {
        if v.is_some_and(|x| !(x.is_finite() && x > 0.0)) {
            return Err(ApiError::bad_request(&format!("{what} must be a positive number")));
        }
    }
    if let Some(tz) = &m.timezone {
        if otw_script::stdlib::time::zone(tz).is_none() {
            return Err(ApiError::bad_request(&format!("unknown timezone {tz} (use an IANA name such as America/New_York)")));
        }
    }
    if let Some(s) = &m.session {
        if !valid_session(s) {
            return Err(ApiError::bad_request("session must look like 0930-1600, 0930-1600:23456 or 24x7"));
        }
    }
    Ok(())
}

/// `HHMM-HHMM[:days]`, comma separated, days 1-7 (Sunday = 1); or `24x7`.
fn valid_session(s: &str) -> bool {
    let s = s.trim();
    if s.eq_ignore_ascii_case("24x7") {
        return true;
    }
    let hhmm = |x: &str| x.len() == 4 && x.bytes().all(|b| b.is_ascii_digit()) && x[..2] < *"24" && x[2..] < *"60";
    !s.is_empty()
        && s.split(',').all(|part| {
            let (range, days) = part.split_once(':').unwrap_or((part, "1"));
            let ok_days = !days.is_empty() && days.bytes().all(|b| (b'1'..=b'7').contains(&b));
            ok_days && range.split_once('-').is_some_and(|(a, b)| hhmm(a) && hhmm(b))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> InstrumentMeta {
        InstrumentMeta {
            provider: "p".into(),
            asset_type: "equity".into(),
            ticker: "AAPL".into(),
            mintick: None,
            pointvalue: None,
            timezone: None,
            session: None,
            kind: None,
            currency: None,
            basecurrency: None,
            description: None,
            margin: None,
            margin_notional: None,
        }
    }

    #[test]
    fn all_unknown_is_fine() {
        assert!(validate_instrument_meta(&meta()).is_ok());
    }

    #[test]
    fn refuses_bad_numbers() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(validate_instrument_meta(&InstrumentMeta { mintick: Some(bad), ..meta() }).is_err(), "mintick {bad}");
            assert!(validate_instrument_meta(&InstrumentMeta { pointvalue: Some(bad), ..meta() }).is_err(), "pointvalue {bad}");
        }
    }

    #[test]
    fn timezone_must_exist() {
        assert!(validate_instrument_meta(&InstrumentMeta { timezone: Some("Mars/Base".into()), ..meta() }).is_err());
        assert!(validate_instrument_meta(&InstrumentMeta { timezone: Some("America/New_York".into()), ..meta() }).is_ok());
        assert!(validate_instrument_meta(&InstrumentMeta { timezone: Some("Etc/UTC".into()), ..meta() }).is_ok());
    }

    #[test]
    fn session_format() {
        for ok in ["0930-1600", "0930-1600:23456", "24x7", "0930-1200,1300-1600"] {
            assert!(validate_instrument_meta(&InstrumentMeta { session: Some(ok.into()), ..meta() }).is_ok(), "{ok}");
        }
        for bad in ["25:00", "2500-1600", "0930-1600:8", "x"] {
            assert!(validate_instrument_meta(&InstrumentMeta { session: Some(bad.into()), ..meta() }).is_err(), "{bad}");
        }
    }

    #[test]
    fn instrument_is_required() {
        assert!(validate_instrument_meta(&InstrumentMeta { ticker: " ".into(), ..meta() }).is_err());
    }
}
