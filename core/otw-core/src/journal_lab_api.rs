//! HTTP API of the journal's what-if lab and of the strategy report card.
//!
//! Every route here reads only: a scenario replays the trader's own closed trades, never
//! writes, never fetches. The exit lab keeps its own routes (`/api/journal/exits/*`, also
//! mounted under `/api/journal/lab/exits/*` in `journal_api`); `/api/journal/lab/simulate`
//! runs any of the five kinds, so a researcher can hand the lab one [`Scenario`].

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::journal_api::{CalendarQuery, TradeQuery};
use crate::journal_exits;
use crate::journal_whatif::{self as whatif, Book, CardRun, Refused, Scenario, Scope};
use crate::{ApiError, AppState};
use otw_store::journal::{self, TradeFilter};

/// Trades one run accepts when they are named by id.
pub const MAX_IDS: usize = 5_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/journal/lab/catalog", get(catalog))
        .route("/api/journal/lab/simulate", post(simulate))
        .route("/api/journal/strategies/{id}/report", get(strategy_report))
}

fn refusal(e: anyhow::Error) -> ApiError {
    match e.downcast_ref::<Refused>() {
        Some(r) => ApiError::bad_request(&r.0),
        None => ApiError::from(e),
    }
}

/// What the lab can run: the kinds, the size bounds, the contract families, the floors.
async fn catalog() -> Json<Value> {
    Json(json!({
        "kinds": whatif::KINDS,
        "size": { "min_factor": whatif::MIN_FACTOR, "max_factor": whatif::MAX_FACTOR },
        "families": whatif::families(),
        "min_sample": whatif::MIN_SAMPLE,
        "max_ids": MAX_IDS,
        "max_bar_trades": whatif::MAX_BAR_TRADES,
        "exits": {
            "rules": journal_exits::RULES,
            "max_trades": journal_exits::MAX_TRADES,
            "max_signals": journal_exits::MAX_SIGNALS,
        },
    }))
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct LabBody {
    /// Closed trades to run the scenario on. Omitted = every closed trade matching the
    /// query filters (same filters as /api/journal/trades).
    #[serde(default)]
    pub trade_ids: Option<Vec<Uuid>>,
    pub scenario: Scenario,
}

/// The body's own refusals, before anything is read.
fn check_body(body: &LabBody) -> Result<(), String> {
    if let Some(ids) = &body.trade_ids {
        if ids.is_empty() {
            return Err("no trade selected: pick trades or leave trade_ids out to use the filters".into());
        }
        let cap = if matches!(body.scenario, Scenario::Exits(_)) { journal_exits::MAX_TRADES } else { MAX_IDS };
        if ids.len() > cap {
            return Err(format!("at most {cap} trades per run ({} given): pick fewer or use the filters", ids.len()));
        }
    }
    whatif::validate(&body.scenario)
}

/// Run one scenario over the scope. Exits answers the exit lab's report; the four others
/// answer an [`whatif::Outcome`]. Reads only.
async fn simulate(
    State(state): State<AppState>,
    Query(q): Query<TradeQuery>,
    Query(cal): Query<CalendarQuery>,
    Json(body): Json<LabBody>,
) -> Result<Json<Value>, ApiError> {
    check_body(&body).map_err(|e| ApiError::bad_request(&e))?;
    let filter = q.into_filter()?;
    let currency = journal::get_settings(&state.pool).await?.display_currency;
    let tz_offset = cal.tz_offset.clamp(-840, 840);
    let LabBody { trade_ids, scenario } = body;

    if let Scenario::Exits(p) = scenario {
        let ids = match trade_ids {
            Some(ids) => ids,
            None => ready_ids(&state, &filter).await?,
        };
        if ids.is_empty() {
            return Err(ApiError::bad_request(
                "no closed trade in this scope has its candles in store: run Market data (sync, then compute) first",
            ));
        }
        let report = journal_exits::simulate(&state.pool, p.into_body(ids), &currency)
            .await
            .map_err(|e| ApiError::bad_request(&e.to_string()))?;
        return Ok(Json(json!({ "kind": "exits", "report": report })));
    }

    if let Scenario::Fees(p) = &scenario {
        if journal::get_fee_schedule(&state.pool, p.schedule_id).await?.is_none() {
            return Err(ApiError::bad_request_code(
                "no_fee_schedule",
                "fee schedule not found: add one under Journal, Fees, then pick it in the lab",
            ));
        }
    }
    let scope = match &trade_ids {
        Some(ids) => Scope::Ids(ids),
        None => Scope::Filter(&filter),
    };
    let book = whatif::load(&state.pool, scope, &currency, tz_offset).await?;
    let outcome = whatif::run(&state.pool, &book, &scenario, &currency).await.map_err(refusal)?;
    Ok(Json(json!({ "kind": scenario.kind(), "outcome": outcome })))
}

/// The scope's trades the exit lab can replay, newest exit first, capped at its ceiling.
async fn ready_ids(state: &AppState, filter: &TradeFilter) -> Result<Vec<Uuid>, ApiError> {
    let ready = journal_exits::ready(&state.pool, filter).await?;
    Ok(ready.trades.iter().take(journal_exits::MAX_TRADES).map(|t| t.id).collect())
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub(crate) struct ReportQuery {
    /// Only this category's trades of the strategy.
    pub category_id: Option<Uuid>,
    /// Viewer's offset (minutes, as `Date.getTimezoneOffset`): hours and days are local.
    #[serde(default)]
    pub tz_offset: i32,
    /// Daily loss cap of the size scenario, in the display currency. Omitted = no cap.
    pub loss_cap: Option<f64>,
}

fn check_report(q: &ReportQuery) -> Result<(), String> {
    match q.loss_cap {
        Some(c) if !c.is_finite() || c <= 0.0 => {
            Err("loss_cap must be a positive amount in the display currency, or left out".into())
        }
        _ => Ok(()),
    }
}

/// One strategy's report card: expectancy, R distribution, by hour, weekday, regime and
/// instrument, and the four standard scenarios run on its trades. Reads only.
async fn strategy_report(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ReportQuery>,
) -> Result<Json<Value>, ApiError> {
    check_report(&q).map_err(|e| ApiError::bad_request(&e))?;
    let strategy = journal::list_strategies(&state.pool)
        .await?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| ApiError::not_found("strategy not found: it may have been deleted, reload the Strategies tab"))?;
    let currency = journal::get_settings(&state.pool).await?.display_currency;
    let filter = TradeFilter { strategy_id: Some(id), category_id: q.category_id, ..Default::default() };
    let book: Book =
        whatif::load(&state.pool, Scope::Filter(&filter), &currency, q.tz_offset.clamp(-840, 840)).await?;
    let stats = whatif::card_stats(&book.trades);

    let mut runs = Vec::new();
    for s in whatif::standard_scenarios(&book.trades, q.loss_cap) {
        let run = match s.scenario.clone() {
            None => {
                let why = s.missing.unwrap_or("no_data");
                CardRun::missing(s, why)
            }
            Some(Scenario::Exits(p)) => {
                let ids = ready_ids(&state, &filter).await?;
                if ids.is_empty() {
                    CardRun::missing(s, "no_bars")
                } else {
                    let mut body = p.into_body(ids);
                    body.view = Some("summary".into());
                    let report = journal_exits::simulate(&state.pool, body, &currency)
                        .await
                        .map_err(|e| ApiError::bad_request(&e.to_string()))?;
                    CardRun::from_exits(s, &report)
                }
            }
            Some(sc) => {
                let o = whatif::run(&state.pool, &book, &sc, &currency).await.map_err(refusal)?;
                CardRun::from_outcome(s, &o)
            }
        };
        runs.push(run);
    }
    Ok(Json(json!({
        "card": {
            "strategy": { "id": strategy.id, "name": strategy.name },
            "currency": currency,
            "min_sample": whatif::MIN_SAMPLE,
            "loss_cap": q.loss_cap,
            "stats": stats,
            "scenarios": runs,
            "unmeasured": book.unmeasured,
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(v: Value) -> LabBody {
        serde_json::from_value(v).expect("valid body")
    }

    #[test]
    fn routes_build() {
        let _ = routes();
    }

    #[test]
    fn a_body_names_its_kind_and_an_unknown_one_is_refused() {
        assert!(serde_json::from_value::<LabBody>(json!({ "scenario": { "kind": "martingale" } })).is_err());
        let b = body(json!({ "scenario": { "kind": "reverse" } }));
        assert_eq!(b.scenario.kind(), "reverse");
        assert!(check_body(&b).is_ok());
    }

    #[test]
    fn an_empty_or_oversized_pick_is_refused_with_the_fix() {
        let b = body(json!({ "trade_ids": [], "scenario": { "kind": "reverse" } }));
        assert!(check_body(&b).unwrap_err().contains("no trade selected"));
        let many: Vec<String> = (0..=journal_exits::MAX_TRADES).map(|_| Uuid::new_v4().to_string()).collect();
        let b = body(json!({ "trade_ids": many, "scenario": { "kind": "exits" } }));
        assert!(check_body(&b).unwrap_err().contains("at most 500"));
        // The same pick is fine for a scenario that reads no candle.
        let b = body(json!({ "trade_ids": many, "scenario": { "kind": "size", "factor": 1.5 } }));
        assert!(check_body(&b).is_ok());
    }

    #[test]
    fn scenario_parameters_are_checked_in_the_handler() {
        let b = body(json!({ "scenario": { "kind": "size", "factor": 0.1 } }));
        assert!(check_body(&b).unwrap_err().contains("between 0.5 and 3"));
        let b = body(json!({ "scenario": { "kind": "filter" } }));
        assert!(check_body(&b).unwrap_err().contains("needs an indicator condition"));
        let b = body(json!({ "scenario": { "kind": "fees", "schedule_id": Uuid::nil(), "family": "nope" } }));
        assert!(check_body(&b).unwrap_err().contains("lab catalog"));
        let b = body(json!({ "scenario": { "kind": "fees", "schedule_id": Uuid::nil(), "family": "mes_es", "to": "small" } }));
        assert!(check_body(&b).is_ok());
    }

    #[test]
    fn a_report_loss_cap_must_be_positive() {
        let q = |cap: Option<f64>| ReportQuery { category_id: None, tz_offset: 0, loss_cap: cap };
        assert!(check_report(&q(None)).is_ok());
        assert!(check_report(&q(Some(250.0))).is_ok());
        assert!(check_report(&q(Some(0.0))).is_err());
        assert!(check_report(&q(Some(f64::NAN))).is_err());
    }

    #[test]
    fn a_refusal_is_a_400_and_anything_else_is_not() {
        let e = refusal(Refused("narrow the filters".into()).into());
        assert_eq!(e.status_code(), axum::http::StatusCode::BAD_REQUEST);
        let e = refusal(anyhow::anyhow!("database went away"));
        assert_ne!(e.status_code(), axum::http::StatusCode::BAD_REQUEST);
    }
}
