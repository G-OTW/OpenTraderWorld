//! HTTP API for journal FX: pending tasks (dates no source could supply) and manual rate
//! entry to resolve them. Read endpoints back the breakdown's "Pending tasks" page.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{ApiError, AppState};
use otw_store::journal::CURRENCIES;
use otw_store::journal_fx::{self, FX_QUOTES};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/journal/fx/pending", get(list_pending))
        .route("/api/journal/fx/quotes", get(quotes))
        .route("/api/journal/fx/rates/{date}", get(rates_on).post(resolve))
}

async fn list_pending(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let pending = journal_fx::list_pending(&state.pool).await?;
    Ok(Json(json!({ "pending": pending })))
}

/// The currencies the user must supply a rate for (USD base, the 11 non-USD majors).
async fn quotes(State(_): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "base": "USD", "quotes": FX_QUOTES })))
}

#[derive(Deserialize)]
struct RatesQuery {
    /// `?asof=true` carries each quote forward from its last known date at or before `date`,
    /// for callers that need a usable rate (weekends, holidays, dates the job hasn't
    /// reached). Default is exact-date, which the manual-resolve UI relies on to tell
    /// "stored for this date" apart from "inherited".
    #[serde(default)]
    asof: bool,
}

async fn rates_on(
    State(state): State<AppState>,
    Path(date): Path<String>,
    Query(q): Query<RatesQuery>,
) -> Result<Json<Value>, ApiError> {
    let d = journal_fx::parse_date(&date).map_err(|_| ApiError::bad_request("invalid date"))?;
    let rates = if q.asof {
        journal_fx::rates_asof(&state.pool, d).await?
    } else {
        journal_fx::rates_on(&state.pool, d).await?
    };
    Ok(Json(json!({ "rates": rates })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ResolveBody {
    /// USD-based rates: { "EUR": 0.92, "GBP": 0.78, ... } for the 11 non-USD majors.
    rates: std::collections::HashMap<String, f64>,
}

/// Manually supply USD-based rates for a date, clearing its pending task. Each quote must
/// be one of the tracked majors and strictly positive. Source is recorded as 'manual'.
async fn resolve(
    State(state): State<AppState>,
    Path(date): Path<String>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<Value>, ApiError> {
    let d = journal_fx::parse_date(&date).map_err(|_| ApiError::bad_request("invalid date"))?;

    let mut rows: Vec<(String, f64)> = Vec::new();
    for (quote, rate) in body.rates {
        if quote == "USD" {
            continue; // base is implicit
        }
        if !CURRENCIES.contains(&quote.as_str()) {
            return Err(ApiError::bad_request("unsupported currency in rates"));
        }
        if !(rate.is_finite() && rate > 0.0) {
            return Err(ApiError::bad_request("each rate must be a positive number"));
        }
        rows.push((quote, rate));
    }
    if rows.is_empty() {
        return Err(ApiError::bad_request("provide at least one rate"));
    }
    journal_fx::upsert_rates(&state.pool, d, &rows, "manual").await?;
    // upsert clears the pending row when rates were stored; this also covers the explicit
    // "I've handled this date" case.
    journal_fx::clear_pending(&state.pool, d).await?;
    Ok(Json(json!({ "ok": true })))
}
