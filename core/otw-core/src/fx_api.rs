//! HTTP API for journal FX: pending tasks and the three ways to resolve one.
//!
//! A task is a **(date, currency)** pair, because that is what is actually missing: a date
//! is never missing as a whole, a currency on it is. The resolution chain the UI drives is
//! the same one the background job walks: the online source (majors only), then a stored
//! histdata series when a market exists for the pair, then manual entry. A currency no
//! market prices is not an error and not a guess, it is a number the user types.
//!
//! Between the two, the user can **name the market**: a currency the conventional tickers
//! cannot reach (USDT, USDC, EURC, any crypto quote asset) gets one stated source, a
//! connector plus the pair as that venue writes it, kept per currency in
//! `journal_fx_sources`. Set once, it serves every date of that currency: the series is
//! read from it and the download goes to it.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use crate::journal_market::MODULE;
use crate::{fx_histdata, histdata, ApiError, AppState};
use otw_store::connectors as conn_store;
use otw_store::histdata as hd_store;
use otw_store::journal_fx::{self, FxSource};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/journal/fx/pending", get(list_pending))
        .route("/api/journal/fx/quotes", get(quotes))
        .route("/api/journal/fx/rates/{date}", get(rates_on).post(resolve))
        .route(
            "/api/journal/fx/pending/{date}/{quote}/histdata",
            post(from_histdata),
        )
        .route(
            "/api/journal/fx/pending/{date}/{quote}/download",
            post(download),
        )
        .route("/api/journal/fx/sources", get(list_sources))
        .route(
            "/api/journal/fx/sources/{quote}",
            get(source_options).put(set_source).delete(clear_source),
        )
}

/// Daily candles: what an FX rate is, and the grain [`fx_histdata`] reads.
const TF: &str = "1d";

/// The asset types an FX source can live under. A pair is either a real FX market or a
/// crypto one; nothing else prices a currency.
const SOURCE_ASSET_TYPES: [&str; 2] = ["fx", "crypto"];

/// Pending tasks, each carrying what can still be tried on it: whether a stored series
/// prices it right now, and whether a download could go and get one. Without those the UI
/// would have to offer every action on every row and let most of them fail.
async fn list_pending(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let pending = journal_fx::list_pending(&state.pool).await?;
    // Memoized per currency: the answer depends on the currency, not on the date, and a
    // book with a hundred pending dates asks the same question a hundred times otherwise.
    let mut can_download: HashMap<String, bool> = HashMap::new();
    let mut sources: HashMap<String, Option<FxSource>> = HashMap::new();
    let mut rows = Vec::with_capacity(pending.len());
    for p in &pending {
        let derived = fx_histdata::derive(&state.pool, &p.quote, p.pending_date)
            .await
            .ok()
            .flatten();
        let downloadable = match can_download.get(&p.quote) {
            Some(v) => *v,
            None => {
                let v = !fx_histdata::downloadable(&state.pool, &p.quote)
                    .await
                    .unwrap_or_default()
                    .is_empty();
                can_download.insert(p.quote.clone(), v);
                v
            }
        };
        let source = match sources.get(&p.quote) {
            Some(v) => v.clone(),
            None => {
                let v = journal_fx::get_source(&state.pool, &p.quote).await.ok().flatten();
                sources.insert(p.quote.clone(), v.clone());
                v
            }
        };
        rows.push(json!({
            "pending_date": p.pending_date.to_string(),
            "quote": p.quote,
            "reason": p.reason,
            "source": source,
            "histdata_ticker": derived.as_ref().map(|d| d.ticker.clone()),
            "histdata_rate": derived.as_ref().map(|d| d.rate),
            "histdata_proxy": derived.as_ref().map(|d| d.proxy).unwrap_or(false),
            "can_download": downloadable,
        }));
    }
    Ok(Json(json!({ "pending": rows })))
}

/// The currencies that can hold a rate: everything the journal is actually booked in, plus
/// the majors. Not a fixed list — a user trading USDT needs USDT here.
async fn quotes(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let quotes = journal_fx::required_quotes(&state.pool).await?;
    Ok(Json(json!({ "base": "USD", "quotes": quotes })))
}

#[derive(Deserialize)]
struct RatesQuery {
    /// `?asof=true` carries each quote forward from its last known date at or before
    /// `date`, for callers that need a usable rate (weekends, holidays, dates the job
    /// hasn't reached). Default is exact-date, which the manual-resolve UI relies on to
    /// tell "stored for this date" apart from "inherited".
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
    /// USD-based rates: `{ "USDT": 1.0, "EUR": 0.92 }`. One entry is enough; a task is one
    /// currency, so the form no longer asks for eleven to fix one.
    rates: HashMap<String, f64>,
}

/// Manually supply USD-based rates for a date, clearing those currencies' pending tasks.
/// Each quote must be a currency the journal actually uses (or a major) and strictly
/// positive. Source is recorded as 'manual', which outranks every other source.
async fn resolve(
    State(state): State<AppState>,
    Path(date): Path<String>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<Value>, ApiError> {
    let d = journal_fx::parse_date(&date).map_err(|_| ApiError::bad_request("invalid date"))?;
    let allowed = journal_fx::required_quotes(&state.pool).await?;

    let mut rows: Vec<(String, f64)> = Vec::new();
    for (quote, rate) in body.rates {
        let quote = quote.trim().to_uppercase();
        if quote == "USD" {
            continue; // base is implicit
        }
        if !allowed.contains(&quote) {
            return Err(ApiError::bad_request(&format!(
                "{quote} is not a currency this journal holds anything in"
            )));
        }
        if !(rate.is_finite() && rate > 0.0) {
            return Err(ApiError::bad_request("each rate must be a positive number"));
        }
        rows.push((quote, rate));
    }
    if rows.is_empty() {
        return Err(ApiError::bad_request("provide at least one rate"));
    }
    // The upsert clears exactly the tasks it answered, and no others: storing EUR says
    // nothing about whether USDT is still missing on the same date.
    journal_fx::upsert_rates(&state.pool, d, &rows, "manual").await?;
    Ok(Json(json!({ "ok": true, "stored": rows.len() })))
}

/// Resolve one task from a stored histdata series. 404 when no stored market prices this
/// currency, which is the UI's cue to offer a download or the manual field.
async fn from_histdata(
    State(state): State<AppState>,
    Path((date, quote)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let d = journal_fx::parse_date(&date).map_err(|_| ApiError::bad_request("invalid date"))?;
    let quote = quote.trim().to_uppercase();
    match fx_histdata::resolve(&state.pool, &quote, d).await? {
        Some(r) => Ok(Json(json!({
            "ok": true,
            "rate": r.rate,
            "ticker": r.ticker,
            "proxy": r.proxy,
        }))),
        None => Err(ApiError::not_found(
            "no stored market prices this currency on that date: download one or enter the rate",
        )),
    }
}

/// Queue the download of a series that could price this currency.
async fn download(
    State(state): State<AppState>,
    Path((date, quote)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let d = journal_fx::parse_date(&date).map_err(|_| ApiError::bad_request("invalid date"))?;
    let quote = quote.trim().to_uppercase();
    let (job_id, ticker) = fx_histdata::enqueue(&state.pool, &quote, d)
        .await
        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
    Ok(Json(json!({ "ok": true, "job_id": job_id, "ticker": ticker })))
}

// ── Stated sources ────────────────────────────────────────────────────────────

/// Every currency the user has pointed at a market, so the settings screen can list them
/// without walking the pending tasks.
async fn list_sources(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let sources = journal_fx::list_sources(&state.pool).await?;
    Ok(Json(json!({ "sources": sources })))
}

/// What this currency can be pointed at: the mapping it already has, the connectors granted
/// to the journal that could fetch a pair, and the daily series already in store whose
/// ticker names the currency. The last one is why the form is usually a single click: a
/// user who downloaded USDTUSD for a chart has already answered the question.
async fn source_options(
    State(state): State<AppState>,
    Path(quote): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let quote = quote.trim().to_uppercase();
    let source = journal_fx::get_source(&state.pool, &quote).await?;

    let connectors: Vec<Value> = conn_store::list_for_module(&state.pool, MODULE)
        .await?
        .into_iter()
        .filter_map(|c| {
            // Only the asset types this provider actually serves at the daily grain: a
            // greyed-out option is better than a job queued and then refused.
            let types: Vec<&str> = SOURCE_ASSET_TYPES
                .into_iter()
                .filter(|t| histdata::validate_request(&c.provider, t, TF).is_ok())
                .collect();
            if types.is_empty() {
                return None;
            }
            let label = histdata::connector_for(&c.provider)
                .map(|k| k.capability().label.to_string())
                .unwrap_or_else(|_| c.provider.clone());
            Some(json!({
                "id": c.id,
                "name": c.name,
                "provider": c.provider,
                "label": label,
                "asset_types": types,
            }))
        })
        .collect();

    // Substring match on the ticker, not a parse: a venue writes the pair its own way
    // (USDTUSD, USDT-USD, USDT/USD) and the user confirms the row by picking it.
    let datasets: Vec<Value> = hd_store::list_datasets(&state.pool)
        .await?
        .into_iter()
        .filter(|d| {
            d.timeframe == TF
                && SOURCE_ASSET_TYPES.contains(&d.asset_type.as_str())
                && d.ticker.to_uppercase().contains(&quote)
                && d.bar_count > 0
        })
        .map(|d| {
            json!({
                "provider": d.provider,
                "asset_type": d.asset_type,
                "ticker": d.ticker,
                "bar_count": d.bar_count,
                "range_from": d.range_from.map(|t| t.date().to_string()),
                "range_to": d.range_to.map(|t| t.date().to_string()),
            })
        })
        .collect();

    Ok(Json(
        json!({ "quote": quote, "source": source, "connectors": connectors, "datasets": datasets }),
    ))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SourceBody {
    /// The connector that will fetch the pair. Omitted only when pointing at a series
    /// already in store (an import, or a provider no connector covers any more), in which
    /// case `provider` names it.
    connector_id: Option<Uuid>,
    /// The provider of an already-stored series, when there is no connector.
    #[serde(default)]
    provider: Option<String>,
    /// `fx` or `crypto`: what kind of market the pair is.
    asset_type: String,
    /// The pair exactly as the venue writes it, e.g. `USDTUSD` on Binance.
    ticker: String,
    /// `false`: the ticker quotes the currency per USD (USDJPY). `true`: it quotes USD per
    /// the currency (USDTUSD), so the stored rate is `1 / close`. Whichever is picked, the
    /// other leg is taken as USD: choosing a USDC pair for USDT accepts that peg.
    #[serde(default)]
    invert: bool,
}

/// Point a currency at one market. Last write wins; a currency has one market.
async fn set_source(
    State(state): State<AppState>,
    Path(quote): Path<String>,
    Json(body): Json<SourceBody>,
) -> Result<Json<Value>, ApiError> {
    let quote = quote.trim().to_uppercase();
    if quote == "USD" {
        return Err(ApiError::bad_request("USD is the base and needs no market"));
    }
    let allowed = journal_fx::required_quotes(&state.pool).await?;
    if !allowed.contains(&quote) {
        return Err(ApiError::bad_request(&format!(
            "{quote} is not a currency this journal holds anything in"
        )));
    }
    let ticker = body.ticker.trim().to_string();
    if ticker.is_empty() {
        return Err(ApiError::bad_request("the pair is required"));
    }
    let asset_type = body.asset_type.trim().to_lowercase();
    if !SOURCE_ASSET_TYPES.contains(&asset_type.as_str()) {
        return Err(ApiError::bad_request("asset type must be fx or crypto"));
    }

    let (connector_id, provider) = match body.connector_id {
        Some(id) => {
            let c = conn_store::get(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::not_found("connector not found"))?;
            if !c.allows(MODULE) {
                return Err(ApiError::bad_request(
                    "that connector is not granted to the journal",
                ));
            }
            // The capability matrix, not the UI, decides: a connector that cannot serve
            // this asset type at the daily grain would queue a job and have it refused.
            histdata::validate_request(&c.provider, &asset_type, TF)
                .map_err(|e| ApiError::bad_request(&e.to_string()))?;
            (Some(id), c.provider)
        }
        None => {
            let provider = body.provider.unwrap_or_default().trim().to_string();
            if provider.is_empty() {
                return Err(ApiError::bad_request(
                    "choose a connector, or a series already in store",
                ));
            }
            // No connector means nothing can ever fetch it, so the series has to be there
            // already: an imported file, or a provider that is no longer connected.
            let found = hd_store::find_dataset_any(
                &state.pool,
                &asset_type,
                &ticker,
                TF,
                Some(&provider),
            )
            .await?
            .filter(|d| d.provider == provider);
            if found.is_none() {
                return Err(ApiError::bad_request(&format!(
                    "no stored {TF} series {ticker} from {provider}: pick a connector to \
                     download it instead"
                )));
            }
            (None, provider)
        }
    };

    let source = FxSource { quote, connector_id, provider, asset_type, ticker, invert: body.invert };
    journal_fx::set_source(&state.pool, &source).await?;
    Ok(Json(json!({ "ok": true, "source": source })))
}

/// Forget the mapping and fall back to the conventional tickers. Rates already stored stay.
async fn clear_source(
    State(state): State<AppState>,
    Path(quote): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let quote = quote.trim().to_uppercase();
    journal_fx::delete_source(&state.pool, &quote).await?;
    Ok(Json(json!({ "ok": true })))
}
