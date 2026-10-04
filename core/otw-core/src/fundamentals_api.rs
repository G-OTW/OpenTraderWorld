//! HTTP API for the Fundamentals module: stored macro series and company data, plus the
//! lookups that reach a provider.
//!
//! - `GET    /api/fundamentals/series`                       stored series catalog
//! - `POST   /api/fundamentals/series`                       add (provider, code, category)
//! - `PATCH/DELETE /api/fundamentals/series/{id}`            recategorise / remove
//! - `POST   /api/fundamentals/series/{id}/refresh`          refetch in the background
//! - `GET    /api/fundamentals/series/{id}/observations`     `[[ms, value]]`, oldest first
//! - `GET    /api/fundamentals/starter`                      suggested first series
//! - `GET    /api/fundamentals/sources`                      fundamentals providers + connected
//! - `GET    /api/fundamentals/companies`                    stored companies
//! - `POST   /api/fundamentals/companies`                    open a ticker (resolve, store, fetch)
//! - `GET/PATCH/DELETE /api/fundamentals/companies/{ticker}` profile + metrics / follow or
//!   mark opened / remove
//! - `GET    /api/fundamentals/etfs`                         ETFs the user opened
//! - `PATCH/DELETE /api/fundamentals/etfs/{ticker}`          follow or mark opened / forget
//! - `POST   /api/fundamentals/companies/{ticker}/refresh`
//! - `GET    /api/fundamentals/companies/{ticker}/statements?kind=&freq=annual|quarterly`
//! - `GET    /api/fundamentals/companies/{ticker}/documents?form=`
//! - `GET    /api/fundamentals/companies/{ticker}/insiders`
//! - `GET    /api/fundamentals/documents?q=&form=`           across companies, full text
//! - `GET    /api/fundamentals/lookup/series?provider=&q=`   provider catalog search
//! - `GET    /api/fundamentals/lookup/companies?q=`          EDGAR ticker search
//! - `GET    /api/fundamentals/lookup/yield-curve`           Treasury par curve
//! - `GET    /api/fundamentals/datasets`                     datasets + who can fill them, in
//!   the order they are tried (the user's, else the app's)
//! - `PUT/DELETE /api/fundamentals/datasets/{id}/order`      set / reset that order
//!   (`transcripts` included)
//! - `GET    /api/fundamentals/data/{dataset}?subject=`      stored snapshot (or null)
//! - `POST   /api/fundamentals/data/{dataset}/refresh?subject=&provider=`  fetch + store
//! - `GET    /api/fundamentals/companies/{ticker}/view/{view}` earnings | capital |
//!   ownership | peers | ratings: snapshots joined with the stored statements
//! - `GET    /api/fundamentals/calendar`                     market calendar + bank moves
//! - `POST   /api/fundamentals/companies/{ticker}/transcripts/refresh?provider=`
//! - `GET    /api/fundamentals/documents/{id}`               a document + transcript turns
//! - `POST   /api/fundamentals/documents/{id}/fetch`         fetch a transcript's text
//!
//! Everything under `/lookup` reaches a provider on the spot; everything else reads what
//! is stored. Fetching a series or a company runs in the background and the row carries
//! the outcome (`status`, `error`), so the client polls the GET.

use axum::{
    extract::{Path, Query, State},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::Date;
use uuid::Uuid;

use crate::fundamentals::{self as fund, datasets, edgar, skips, transcripts, treasury, Ctx};
use crate::{ApiError, AppState};
use otw_store::fundamentals as store;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/fundamentals/series", get(list_series).post(add_series))
        .route(
            "/api/fundamentals/series/{id}",
            get(get_series).patch(patch_series).delete(delete_series),
        )
        .route("/api/fundamentals/series/{id}/refresh", post(refresh_series))
        .route("/api/fundamentals/series/{id}/observations", get(observations))
        .route("/api/fundamentals/starter", get(starter))
        .route("/api/fundamentals/sources", get(sources))
        .route("/api/fundamentals/companies", get(list_companies).post(open_company))
        .route(
            "/api/fundamentals/companies/{ticker}",
            get(get_company).patch(patch_company).delete(delete_company),
        )
        .route("/api/fundamentals/companies/{ticker}/refresh", post(refresh_company))
        .route("/api/fundamentals/etfs", get(list_etfs))
        .route("/api/fundamentals/etfs/{ticker}", patch(patch_etf).delete(delete_etf))
        .route("/api/fundamentals/companies/{ticker}/statements", get(statements))
        .route("/api/fundamentals/companies/{ticker}/documents", get(company_documents))
        .route("/api/fundamentals/companies/{ticker}/insiders", get(insiders))
        .route("/api/fundamentals/documents", get(documents))
        .route("/api/fundamentals/lookup/series", get(lookup_series))
        .route("/api/fundamentals/lookup/companies", get(lookup_companies))
        .route("/api/fundamentals/lookup/yield-curve", get(yield_curve))
        .route("/api/fundamentals/datasets", get(list_datasets))
        .route(
            "/api/fundamentals/datasets/{id}/order",
            axum::routing::put(set_order).delete(reset_order),
        )
        .route("/api/fundamentals/data/{dataset}", get(get_data))
        .route("/api/fundamentals/data/{dataset}/refresh", post(refresh_data))
        .route("/api/fundamentals/companies/{ticker}/view/{view}", get(company_view))
        .route("/api/fundamentals/calendar", get(calendar))
        .route("/api/fundamentals/companies/{ticker}/transcripts/refresh", post(refresh_transcripts))
        .route("/api/fundamentals/documents/{id}", get(document))
        .route("/api/fundamentals/documents/{id}/fetch", post(fetch_document))
}

/// A provider failure reaches the user as the provider's own message (it names the fix),
/// not as an opaque 500.
fn upstream(e: anyhow::Error) -> ApiError {
    ApiError::bad_gateway(&format!("{e:#}"))
}

async fn creds(
    state: &AppState,
    provider: &str,
) -> Result<(Uuid, std::collections::HashMap<String, String>), ApiError> {
    fund::creds_for(&state.pool, &state.cipher, provider)
        .await?
        .ok_or_else(|| ApiError::bad_request(&fund::missing_connector(provider)))
}

fn date_param(s: &Option<String>) -> Result<Option<Date>, ApiError> {
    match s.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => fund::parse_date(s)
            .map(Some)
            .ok_or_else(|| ApiError::bad_request("dates are YYYY-MM-DD")),
    }
}

// ── Series ──────────────────────────────────────────────────────────────────

fn series_json(s: &store::SeriesRow) -> Value {
    let label = crate::histdata::connector_for(&s.provider)
        .map(|c| c.capability().label)
        .unwrap_or(&s.provider);
    json!({
        "id": s.id,
        "provider_id": s.provider,
        "provider": label,
        "code": s.code,
        "title": s.title,
        "category": s.category,
        "country": s.country,
        "unit": s.unit,
        "freq": s.frequency,
        "sa": s.seasonal_adj,
        "notes": s.notes,
        "status": s.status,
        "error": s.error,
        "last": s.last,
        "prev": s.prev,
        "last_period": s.last_period.map(fund::date_ms),
        "updated": s.last_refreshed.map(|t| t.unix_timestamp() * 1000),
    })
}

async fn list_series(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = store::list_series(&state.pool).await?;
    Ok(Json(json!({ "series": rows.iter().map(series_json).collect::<Vec<_>>() })))
}

async fn get_series(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let s = store::get_series(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("series not found"))?;
    Ok(Json(json!({ "series": series_json(&s) })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AddSeries {
    /// Provider id (`fred`, `treasury`).
    provider: String,
    /// The provider's own series code (`CPIAUCSL`).
    code: String,
    /// Macro category; default `other`.
    category: Option<String>,
}

fn valid_category(c: &str) -> Result<(), ApiError> {
    if fund::CATEGORIES.contains(&c) {
        Ok(())
    } else {
        Err(ApiError::bad_request(&format!(
            "category must be one of {}",
            fund::CATEGORIES.join(", ")
        )))
    }
}

async fn add_series(
    State(state): State<AppState>,
    Json(b): Json<AddSeries>,
) -> Result<Json<Value>, ApiError> {
    let provider = b.provider.trim();
    let code = b.code.trim();
    if !fund::serves_macro(provider) {
        return Err(ApiError::bad_request(&format!("{provider} serves no macro series")));
    }
    if code.is_empty() {
        return Err(ApiError::bad_request("series code is required"));
    }
    let category = b.category.as_deref().map(str::trim).unwrap_or("other");
    valid_category(category)?;
    let (connector, secrets) = creds(&state, provider).await?;
    // The code is checked against the provider before anything is stored: an unknown id
    // is an error naming it, never a row that fails later.
    fund::bump(&state.pool, connector).await;
    let mut meta = fund::series_meta(provider, &state.http, &secrets, code).await.map_err(upstream)?;
    if b.category.is_some() || meta.category.is_empty() {
        meta.category = category.to_string();
    }
    let id = store::upsert_series(&state.pool, &meta).await?;
    fund::spawn_series_refresh(state.clone(), id, meta.provider.clone(), meta.code.clone());
    let row = store::get_series(&state.pool, id).await?;
    Ok(Json(json!({ "series": row.as_ref().map(series_json) })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PatchSeries {
    category: String,
}

async fn patch_series(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<PatchSeries>,
) -> Result<Json<Value>, ApiError> {
    valid_category(b.category.trim())?;
    if !store::set_series_category(&state.pool, id, b.category.trim()).await? {
        return Err(ApiError::not_found("series not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_series(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_series(&state.pool, id).await? {
        return Err(ApiError::not_found("series not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn refresh_series(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let s = store::get_series(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("series not found"))?;
    creds(&state, &s.provider).await?;
    store::set_series_status(&state.pool, id, "pending", None).await?;
    fund::spawn_series_refresh(state.clone(), id, s.provider, s.code);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ObsQuery {
    /// First period start (YYYY-MM-DD, inclusive).
    from: Option<String>,
    /// Last period start (YYYY-MM-DD, exclusive).
    to: Option<String>,
}

async fn observations(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ObsQuery>,
) -> Result<Json<Value>, ApiError> {
    let obs = store::observations(&state.pool, id, date_param(&q.from)?, date_param(&q.to)?).await?;
    let out: Vec<(i64, f64)> = obs.into_iter().map(|(d, v)| (fund::date_ms(d), v)).collect();
    Ok(Json(json!({ "observations": out })))
}

/// A first set worth having, one click away: (provider, code, category).
const STARTER: &[(&str, &str, &str)] = &[
    ("fred", "A191RL1Q225SBEA", "growth"),
    ("fred", "INDPRO", "growth"),
    ("fred", "RSAFS", "growth"),
    ("fred", "CPIAUCSL", "inflation"),
    ("fred", "PCEPILFE", "inflation"),
    ("fred", "UNRATE", "labour"),
    ("fred", "PAYEMS", "labour"),
    ("fred", "ICSA", "labour"),
    ("fred", "FEDFUNDS", "rates"),
    ("fred", "DGS10", "rates"),
    ("fred", "T10Y2Y", "rates"),
    ("fred", "M2SL", "money"),
    ("fred", "UMCSENT", "surveys"),
    ("fred", "HOUST", "housing"),
    ("fred", "DCOILWTICO", "energy"),
    ("treasury", "debt_to_penny", "fiscal"),
    ("ecb", "HICP/M.U2.N.000000.4D0.ANR", "inflation"),
    ("ecb", "FM/D.U2.EUR.4F.KR.DFR.LEV", "rates"),
    ("eurostat", "namq_10_gdp/Q.CLV_PCH_PRE.SCA.B1GQ.EA21", "growth"),
    ("eurostat", "une_rt_m/M.SA.TOTAL.PC_ACT.T.EA21", "labour"),
    ("cftc", "13874A", "positioning"),
    ("cftc", "088691", "positioning"),
];

async fn starter() -> Json<Value> {
    let rows: Vec<Value> = STARTER
        .iter()
        .map(|(p, c, cat)| json!({ "provider": p, "code": c, "category": cat }))
        .collect();
    Json(json!({ "series": rows }))
}

/// Every provider that serves fundamentals, what it serves, and whether a connector for it
/// is granted to the module.
async fn sources(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let granted = otw_store::connectors::list_for_module(&state.pool, fund::MODULE).await?;
    let out: Vec<Value> = crate::histdata::capabilities()
        .into_iter()
        .filter(|c| !c.fundamentals.is_empty())
        .map(|c| {
            json!({
                "id": c.provider,
                "name": c.label,
                "key": if c.required_secrets.is_empty() { "none" } else { "key" },
                "serves": c.fundamentals,
                "connected": granted.iter().any(|g| g.provider == c.provider),
            })
        })
        .collect();
    Ok(Json(json!({ "sources": out })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct LookupSeries {
    provider: String,
    q: String,
}

async fn lookup_series(
    State(state): State<AppState>,
    Query(q): Query<LookupSeries>,
) -> Result<Json<Value>, ApiError> {
    let provider = q.provider.trim();
    if !fund::serves_macro(provider) {
        return Err(ApiError::bad_request(&format!("{provider} serves no macro series")));
    }
    let (connector, secrets) = creds(&state, provider).await?;
    fund::bump(&state.pool, connector).await;
    let hits = fund::search_series(provider, &state.http, &secrets, q.q.trim())
        .await
        .map_err(upstream)?;
    Ok(Json(json!({ "series": hits })))
}

async fn yield_curve(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let (connector, _) = creds(&state, treasury::PROVIDER).await?;
    fund::bump(&state.pool, connector).await;
    let curve = treasury::yield_curve(&state.http).await.map_err(upstream)?;
    Ok(Json(json!({ "curve": curve })))
}

// ── Companies ───────────────────────────────────────────────────────────────

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

/// EDGAR writes the fiscal year end as MMDD.
fn fiscal_month(mmdd: &str) -> String {
    mmdd.get(..2)
        .and_then(|m| m.parse::<usize>().ok())
        .and_then(|m| MONTHS.get(m.wrapping_sub(1)))
        .map(|m| m.to_string())
        .unwrap_or_default()
}

async fn company_json(state: &AppState, c: &store::CompanyRow) -> Result<Value, ApiError> {
    let metrics = datasets::metrics(&state.pool, c).await?;
    Ok(json!({
        "ticker": c.ticker,
        "name": c.name,
        "cik": c.cik,
        "exchange": c.exchange,
        "currency": c.currency,
        "country": c.country,
        "sector": c.sector,
        "industry": c.industry,
        "sic": c.sic,
        "fiscal_year_end": fiscal_month(&c.fiscal_year_end),
        "website": c.website,
        "description": c.description,
        "followed": c.followed,
        "status": c.status,
        "error": c.error,
        "updated": c.last_refreshed.map(|t| t.unix_timestamp() * 1000),
        "price": metrics.get("price").cloned(),
        "change_pct": metrics.get("change_pct").cloned(),
        "metrics": metrics,
    }))
}

async fn company_row(state: &AppState, ticker: &str) -> Result<store::CompanyRow, ApiError> {
    store::company_by_ticker(&state.pool, &edgar::normalize_ticker(ticker))
        .await?
        .ok_or_else(|| ApiError::not_found("company not stored: open it first"))
}

async fn list_companies(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = store::list_companies(&state.pool).await?;
    let out: Vec<Value> = rows
        .iter()
        .map(|c| {
            json!({
                "ticker": c.ticker, "name": c.name, "exchange": c.exchange,
                "sector": c.sector, "followed": c.followed, "status": c.status,
                "opened": c.opened_at.unix_timestamp() * 1000,
            })
        })
        .collect();
    Ok(Json(json!({ "companies": out })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct OpenCompany {
    ticker: String,
}

/// Resolve a ticker on EDGAR, store the company and fetch it. An already stored company
/// is returned as is (refresh is its own call).
async fn open_company(
    State(state): State<AppState>,
    Json(b): Json<OpenCompany>,
) -> Result<Json<Value>, ApiError> {
    let ticker = edgar::normalize_ticker(&b.ticker);
    if ticker.is_empty() {
        return Err(ApiError::bad_request("ticker is required"));
    }
    if let Some(c) = store::company_by_ticker(&state.pool, &ticker).await? {
        return Ok(Json(json!({ "company": company_json(&state, &c).await? })));
    }
    let (_, settings) = creds(&state, edgar::PROVIDER).await?;
    let listing = edgar::resolve(&state.http, &settings, &ticker)
        .await
        .map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let id = store::ensure_company(&state.pool, &listing.ticker, &listing.cik, &listing.name).await?;
    fund::spawn_company_refresh(state.clone(), id, listing.ticker.clone(), listing.cik.clone());
    let c = company_row(&state, &listing.ticker).await?;
    Ok(Json(json!({ "company": company_json(&state, &c).await? })))
}

async fn get_company(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let c = company_row(&state, &ticker).await?;
    Ok(Json(json!({ "company": company_json(&state, &c).await? })))
}

/// `followed` sets the favourite flag; `opened` moves the symbol to the front of the
/// recent ones. Either or both.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PatchSymbol {
    followed: Option<bool>,
    #[serde(default)]
    opened: bool,
}

async fn patch_company(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
    Json(b): Json<PatchSymbol>,
) -> Result<Json<Value>, ApiError> {
    let ticker = edgar::normalize_ticker(&ticker);
    let mut found = true;
    if let Some(f) = b.followed {
        found &= store::set_followed(&state.pool, &ticker, f).await?;
    }
    if b.opened {
        found &= store::touch_company(&state.pool, &ticker).await?;
    }
    if !found {
        return Err(ApiError::not_found("company not stored"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_company(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let ticker = edgar::normalize_ticker(&ticker);
    if !store::delete_company(&state.pool, &ticker).await? {
        return Err(ApiError::not_found("company not stored"));
    }
    store::delete_snapshots(&state.pool, &ticker).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_etfs(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = store::list_etfs(&state.pool).await?;
    let out: Vec<Value> = rows
        .iter()
        .map(|e| {
            json!({
                "ticker": e.ticker, "name": e.name, "followed": e.followed,
                "opened": e.opened_at.unix_timestamp() * 1000,
            })
        })
        .collect();
    Ok(Json(json!({ "etfs": out })))
}

/// `opened` keeps the ETF once its snapshot is stored; a ticker that never resolved is a 404.
async fn patch_etf(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
    Json(b): Json<PatchSymbol>,
) -> Result<Json<Value>, ApiError> {
    let ticker = edgar::normalize_ticker(&ticker);
    let mut found = true;
    if b.opened {
        found &= store::touch_etf(&state.pool, &ticker).await?;
    }
    if let Some(f) = b.followed {
        found &= store::set_etf_followed(&state.pool, &ticker, f).await?;
    }
    if !found {
        return Err(ApiError::not_found("ETF not stored: open it first"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_etf(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_etf(&state.pool, &edgar::normalize_ticker(&ticker)).await? {
        return Err(ApiError::not_found("ETF not stored"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn refresh_company(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let c = company_row(&state, &ticker).await?;
    creds(&state, edgar::PROVIDER).await?;
    store::set_company_status(&state.pool, c.id, "pending", None).await?;
    fund::spawn_company_refresh(state.clone(), c.id, c.ticker, c.cik);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct StatementsQuery {
    /// income | balance | cashflow
    kind: String,
    /// annual (default) | quarterly
    freq: Option<String>,
}

async fn statements(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
    Query(q): Query<StatementsQuery>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(q.kind.as_str(), "income" | "balance" | "cashflow") {
        return Err(ApiError::bad_request("kind must be income, balance or cashflow"));
    }
    let annual = match q.freq.as_deref().unwrap_or("annual") {
        "annual" => true,
        "quarterly" => false,
        _ => return Err(ApiError::bad_request("freq must be annual or quarterly")),
    };
    let c = company_row(&state, &ticker).await?;
    let rows = store::statements(&state.pool, c.id, &q.kind, annual).await?;
    let out: Vec<Value> = rows
        .iter()
        .map(|s| {
            let label = if s.fiscal_period == "FY" {
                format!("FY{}", s.fiscal_year)
            } else {
                format!("{} FY{}", s.fiscal_period, s.fiscal_year)
            };
            json!({
                "period": label,
                "fiscal_period": s.fiscal_period,
                "fiscal_year": s.fiscal_year,
                "period_start": s.period_start.to_string(),
                "period_end": s.period_end.to_string(),
                "source": s.source,
                "lines": s.lines,
                "tags": s.tags,
            })
        })
        .collect();
    Ok(Json(json!({ "statements": out })))
}

fn doc_json(d: &store::DocumentRow) -> Value {
    json!({
        "id": d.id,
        "ticker": d.ticker,
        "kind": d.kind,
        "form": d.form,
        "accession": d.accession,
        "title": d.title,
        "filed_at": d.filed_at.to_string(),
        "period": d.period.map(|p| p.to_string()),
        "source": d.source,
        "url": d.url,
        "has_body": d.has_body,
    })
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DocQuery {
    /// Form type filter (`10-K`, `8-K`, `4`, ...).
    form: Option<String>,
    /// Full-text search over titles and fetched bodies (cross-company list only).
    q: Option<String>,
    /// Max rows (default 300, cap 2000).
    limit: Option<i64>,
}

async fn company_documents(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
    Query(q): Query<DocQuery>,
) -> Result<Json<Value>, ApiError> {
    let c = company_row(&state, &ticker).await?;
    let limit = q.limit.unwrap_or(300).clamp(1, 2000);
    let form = q.form.as_deref().filter(|f| !f.is_empty());
    let rows = store::documents(&state.pool, Some(c.id), None, form, limit).await?;
    Ok(Json(json!({ "documents": rows.iter().map(doc_json).collect::<Vec<_>>() })))
}

async fn documents(
    State(state): State<AppState>,
    Query(q): Query<DocQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = q.limit.unwrap_or(300).clamp(1, 2000);
    let rows = match q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => store::search_documents(&state.pool, text, limit).await?,
        None => {
            let form = q.form.as_deref().filter(|f| !f.is_empty());
            store::documents(&state.pool, None, None, form, limit).await?
        }
    };
    Ok(Json(json!({ "documents": rows.iter().map(doc_json).collect::<Vec<_>>() })))
}

/// SEC transaction codes, as a reader says them.
fn tx_type(code: &str) -> &'static str {
    match code {
        "P" => "Buy",
        "S" => "Sell",
        "A" => "Grant",
        "M" | "X" => "Exercise",
        "F" => "Tax withholding",
        "G" => "Gift",
        "D" => "Disposition to issuer",
        "C" => "Conversion",
        "J" => "Other",
        _ => "Other",
    }
}

async fn insiders(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let c = company_row(&state, &ticker).await?;
    let rows = store::insider_trades(&state.pool, c.id, 300).await?;
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "date": r.tx_date.to_string(),
                "filed_at": r.filed_at.to_string(),
                "insider": r.insider,
                "role": r.role,
                "code": r.code,
                "type": tx_type(&r.code),
                "acquired": r.acquired,
                "shares": r.shares,
                "price": r.price,
                "value": r.price.map(|p| p * r.shares),
                "owned_after": r.owned_after,
            })
        })
        .collect();
    Ok(Json(json!({ "insiders": out })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct LookupCompanies {
    q: String,
}

async fn lookup_companies(
    State(state): State<AppState>,
    Query(q): Query<LookupCompanies>,
) -> Result<Json<Value>, ApiError> {
    let (_, settings) = creds(&state, edgar::PROVIDER).await?;
    let hits = edgar::search(&state.http, &settings, &q.q, 12).await.map_err(upstream)?;
    let out: Vec<Value> = hits
        .iter()
        .map(|l| json!({ "ticker": l.ticker, "name": l.name, "exchange": l.exchange, "cik": l.cik }))
        .collect();
    Ok(Json(json!({ "companies": out })))
}

// ── Datasets ────────────────────────────────────────────────────────────────

/// Every dataset, which providers can fill it in the order they are tried, and which of
/// them are connected. `custom` = the user reordered it; `default` is the app's order.
async fn list_datasets(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let granted = otw_store::connectors::list_for_module(&state.pool, fund::MODULE).await?;
    let stored = store::provider_orders(&state.pool).await?;
    let has_market = granted.iter().any(|g| {
        crate::histdata::connector_for(&g.provider)
            .map(|c| c.capability().asset_types.contains(&"equity") && c.capability().timeframes.contains(&"1d"))
            .unwrap_or(false)
    });
    let entry = |id: &str, default: &[&str], extra: Value| {
        let order = datasets::merge_order(default, stored.get(id).map(Vec::as_slice));
        let providers: Vec<Value> = order
            .iter()
            .map(|p| {
                let connected = if p == datasets::MARKET { has_market } else { granted.iter().any(|g| &g.provider == p) };
                json!({ "id": p, "label": datasets::label(p), "connected": connected })
            })
            .collect();
        let mut v = json!({
            "id": id,
            "providers": providers,
            "default": default,
            "custom": stored.contains_key(id) && order.iter().map(String::as_str).ne(default.iter().copied()),
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    };
    let out: Vec<Value> = datasets::DATASETS
        .iter()
        .map(|d| entry(d.id, d.providers, json!({ "ttl_hours": d.ttl_hours })))
        .collect();
    let tr = entry(TRANSCRIPTS, transcripts::PROVIDERS, json!({}));
    Ok(Json(json!({ "datasets": out, "transcripts": tr["providers"].clone(), "transcripts_order": tr })))
}

/// The order key of the transcript providers.
const TRANSCRIPTS: &str = "transcripts";

/// The app's own provider list for an orderable dataset. The price takes any market-data
/// connector (the longest history wins), so it has no order to set.
fn default_order(id: &str) -> Result<&'static [&'static str], ApiError> {
    if id == TRANSCRIPTS {
        return Ok(transcripts::PROVIDERS);
    }
    let d = datasets::dataset(id).ok_or_else(|| ApiError::not_found("unknown dataset"))?;
    if d.providers.len() < 2 {
        return Err(ApiError::bad_request("this dataset has a single source: there is no order to set"));
    }
    Ok(d.providers)
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct OrderBody {
    /// Every provider id of the dataset, best first.
    providers: Vec<String>,
}

async fn set_order(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<OrderBody>,
) -> Result<Json<Value>, ApiError> {
    let default = default_order(&id)?;
    let mut given: Vec<&str> = b.providers.iter().map(String::as_str).collect();
    let mut want: Vec<&str> = default.to_vec();
    given.sort_unstable();
    want.sort_unstable();
    if given != want {
        return Err(ApiError::bad_request(&format!(
            "the order must list each of {} exactly once",
            default.join(", ")
        )));
    }
    if b.providers.iter().map(String::as_str).eq(default.iter().copied()) {
        store::delete_provider_order(&state.pool, &id).await?;
    } else {
        store::set_provider_order(&state.pool, &id, &b.providers).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn reset_order(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    default_order(&id)?;
    store::delete_provider_order(&state.pool, &id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DataQuery {
    /// Ticker for company and ETF datasets; `_` for the calendar, central banks and the
    /// latest congress trades.
    subject: Option<String>,
    /// Provider id to use instead of the first connected one (refresh only).
    provider: Option<String>,
    /// A manual refresh: ask providers that refused lately or spent their quota too.
    #[serde(default)]
    force: bool,
}

fn subject_of(q: &DataQuery, dataset: &str) -> Result<String, ApiError> {
    if matches!(dataset, "calendar" | "central_banks") {
        return Ok("_".into());
    }
    // Congress trades are per ticker, or `_` for the latest across all members.
    if dataset == "congress" && matches!(q.subject.as_deref().map(str::trim), None | Some("" | "_")) {
        return Ok("_".into());
    }
    let s = q.subject.as_deref().map(edgar::normalize_ticker).unwrap_or_default();
    if s.is_empty() || s.len() > 20 {
        return Err(ApiError::bad_request("subject (a ticker) is required"));
    }
    Ok(s)
}

fn snapshot_json(s: &store::Snapshot) -> Value {
    json!({
        "dataset": s.dataset,
        "subject": s.subject,
        "provider": s.provider,
        "provider_label": datasets::label(&s.provider),
        "fetched_at": s.fetched_at.unix_timestamp() * 1000,
        "stale": datasets::stale(s),
        "data": s.data,
    })
}

async fn get_data(
    State(state): State<AppState>,
    Path(dataset): Path<String>,
    Query(q): Query<DataQuery>,
) -> Result<Json<Value>, ApiError> {
    datasets::dataset(&dataset).ok_or_else(|| ApiError::not_found("unknown dataset"))?;
    let subject = subject_of(&q, &dataset)?;
    let snap = store::snapshot(&state.pool, &subject, &dataset).await?;
    Ok(Json(json!({ "snapshot": snap.as_ref().map(snapshot_json) })))
}

/// Fetch a dataset now and store it. A provider failure is the provider's message. When
/// an automatic refresh asked no one (each provider refused lately or spent its quota),
/// `deferred` says who and until when, next to the stored snapshot if any.
async fn refresh_data(
    State(state): State<AppState>,
    Path(dataset): Path<String>,
    Query(q): Query<DataQuery>,
) -> Result<Json<Value>, ApiError> {
    datasets::dataset(&dataset).ok_or_else(|| ApiError::not_found("unknown dataset"))?;
    let subject = subject_of(&q, &dataset)?;
    match datasets::refresh(&state, &dataset, &subject, q.provider.as_deref(), q.force)
        .await
        .map_err(upstream)?
    {
        datasets::Refreshed::Stored(snap) => Ok(Json(json!({ "snapshot": snapshot_json(&snap) }))),
        datasets::Refreshed::Deferred(skips) => {
            let snap = store::snapshot(&state.pool, &subject, &dataset).await?;
            Ok(Json(json!({ "snapshot": snap.as_ref().map(snapshot_json), "deferred": skips })))
        }
    }
}

async fn company_view(
    State(state): State<AppState>,
    Path((ticker, view)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let c = company_row(&state, &ticker).await?;
    let v = match view.as_str() {
        "earnings" => datasets::earnings_view(&state.pool, &c).await?,
        "capital" => datasets::capital_view(&state.pool, &c).await?,
        "ownership" => datasets::ownership_view(&state.pool, &c).await?,
        "peers" => datasets::peers_view(&state.pool, &c).await?,
        "ratings" => datasets::ratings_view(&state.pool, &c).await?,
        _ => return Err(ApiError::not_found("unknown view")),
    };
    Ok(Json(v))
}

async fn calendar(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(datasets::calendar_view(&state.pool).await?))
}

// ── Transcripts ─────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ProviderQuery {
    /// Provider id; default the first connected transcript provider.
    provider: Option<String>,
    /// A manual refresh: ask providers that refused lately or spent their quota too.
    #[serde(default)]
    force: bool,
}

async fn transcript_creds(
    state: &AppState,
    asked: Option<&str>,
) -> Result<(String, Uuid, std::collections::HashMap<String, String>), ApiError> {
    let list: Vec<&str> = match asked {
        Some(p) if transcripts::PROVIDERS.contains(&p) => vec![p],
        Some(p) => return Err(ApiError::bad_request(&format!("{p} does not serve transcripts"))),
        None => transcripts::PROVIDERS.to_vec(),
    };
    for p in list {
        if let Some((id, secrets)) = fund::creds_for(&state.pool, &state.cipher, p).await? {
            return Ok((p.to_string(), id, secrets));
        }
    }
    Err(ApiError::bad_request(
        "no transcript provider is granted to Fundamentals: add Financial Modeling Prep, Alpha \
         Vantage or Finnhub from the data broker",
    ))
}

async fn refresh_transcripts(
    State(state): State<AppState>,
    Path(ticker): Path<String>,
    Query(q): Query<ProviderQuery>,
) -> Result<Json<Value>, ApiError> {
    let c = company_row(&state, &ticker).await?;
    let list: Vec<String> = match q.provider.as_deref() {
        Some(p) if transcripts::PROVIDERS.contains(&p) => vec![p.to_string()],
        Some(p) => return Err(ApiError::bad_request(&format!("{p} does not serve transcripts"))),
        None => datasets::effective_order(&state.pool, TRANSCRIPTS, transcripts::PROVIDERS).await?,
    };
    // Best provider first (the user's order when set); one whose plan leaves transcripts
    // out hands over to the next, and is not asked again by the next automatic listing
    // until its refusal expires.
    let force = q.force || q.provider.is_some();
    let recent = skips::failures(&state.pool, TRANSCRIPTS, &c.ticker, force).await?;
    let mut failures = Vec::new();
    let mut deferred = Vec::new();
    for p in list.iter().map(String::as_str) {
        let Some((connector, secrets)) = fund::creds_for(&state.pool, &state.cipher, p).await? else { continue };
        if let Some(skip) = skips::check(&state.pool, &recent, p, connector, force).await {
            deferred.push(skip);
            continue;
        }
        fund::bump(&state.pool, connector).await;
        let ctx = Ctx { client: &state.http, secrets: &secrets };
        match transcripts::refresh(&state.pool, &ctx, p, c.id, &c.ticker, &c.name).await {
            Ok(n) => {
                skips::clear(&state.pool, TRANSCRIPTS, &c.ticker, p).await;
                return Ok(Json(json!({ "ok": true, "listed": n, "provider": datasets::label(p) })));
            }
            Err(e) => {
                let msg = skips::redact(&format!("{e:#}"), &secrets);
                skips::record(&state.pool, TRANSCRIPTS, &c.ticker, p, &msg).await;
                failures.push(format!("{}: {msg}", datasets::label(p)));
            }
        }
    }
    if failures.is_empty() && !deferred.is_empty() {
        return Ok(Json(json!({ "ok": true, "listed": 0, "deferred": deferred })));
    }
    failures.extend(deferred.iter().map(|s| format!("{}: not asked, to spare its quota", s.label)));
    if failures.is_empty() {
        return Err(ApiError::bad_request(
            "no transcript provider is granted to Fundamentals: add Financial Modeling Prep, Alpha \
             Vantage or Finnhub from the data broker",
        ));
    }
    Err(ApiError::bad_gateway(&failures.join(" | ")))
}

async fn document(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let d = store::document(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("document not found"))?;
    let segments = store::segments(&state.pool, id).await?;
    let words = store::body_words(&state.pool, id).await?;
    let mut out = doc_json(&d);
    out["words"] = json!(words);
    out["segments"] = json!(segments);
    Ok(Json(json!({ "document": out })))
}

async fn fetch_document(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let d = store::document(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("document not found"))?;
    if d.kind != "transcript" {
        return Err(ApiError::bad_request("only transcripts are fetched here; a filing opens at its source"));
    }
    let provider = transcripts::provider_of(&d.accession)
        .ok_or_else(|| ApiError::bad_request("this transcript names no provider"))?;
    let (_, connector, secrets) = transcript_creds(&state, Some(provider)).await?;
    fund::bump(&state.pool, connector).await;
    let ctx = Ctx { client: &state.http, secrets: &secrets };
    transcripts::fetch_body(&state.pool, &ctx, id, &d.accession)
        .await
        .map_err(|e| ApiError::bad_gateway(&skips::redact(&format!("{e:#}"), &secrets)))?;
    document(State(state), Path(id)).await
}
