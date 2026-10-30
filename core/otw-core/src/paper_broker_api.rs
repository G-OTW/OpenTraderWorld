//! HTTP API for a paper broker account's money (`brokers::paper`). Orders, positions and fills
//! go through the broker routes like any account's; this is what only a paper account has.
//!
//! - `GET  /api/brokers/{id}/paper`        cash, equity, buying power, cash moves, recent orders
//! - `POST /api/brokers/{id}/paper/cash`   deposit | withdraw | reset (`{kind, amount?}`)
//!
//! Under `/api/brokers`, so demo mode denies it with the rest. Not in the MCP catalog: moving
//! an account's money is the user's.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use otw_store::brokers as accounts;
use otw_store::paper_broker as store;

use crate::brokers::paper;
use crate::{ApiError, AppState};

/// Orders that stopped working, listed with the account.
const HISTORY: i64 = 50;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/brokers/{id}/paper", get(overview))
        .route("/api/brokers/{id}/paper/cash", post(cash))
}

async fn paper_account(state: &AppState, id: Uuid) -> Result<accounts::BrokerRow, ApiError> {
    let a = accounts::get(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("broker account not found"))?;
    if a.broker != paper::ID {
        return Err(ApiError::bad_request(&format!("“{}” is not a paper account", a.name)));
    }
    Ok(a)
}

async fn overview(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let a = paper_account(&state, id).await?;
    let summary = paper::summary(&state.pool, &a).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    let moves = store::cash_moves(&state.pool, id).await?;
    let history = store::closed_orders(&state.pool, id, HISTORY).await?;
    Ok(Json(json!({ "summary": summary, "cash": moves, "history": history })))
}

#[derive(Deserialize)]
struct CashBody {
    /// deposit | withdraw | reset
    kind: String,
    /// In the account's currency, as a decimal. A reset without one goes back to the starting
    /// balance.
    #[serde(default, deserialize_with = "amount_of")]
    amount: Option<otw_connector_model::Decimal>,
}

/// The page sends a number, a script may send a decimal string: either is read as the decimal
/// it spells, never through a float sum.
fn amount_of<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<otw_connector_model::Decimal>, D::Error> {
    use serde::de::Error;
    match Option::<Value>::deserialize(d)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.to_string().parse().map(Some).map_err(D::Error::custom),
        Some(Value::String(s)) => s.trim().parse().map(Some).map_err(|_| D::Error::custom(format!("{s:?} is not an amount"))),
        Some(other) => Err(D::Error::custom(format!("{other} is not an amount"))),
    }
}

async fn cash(State(state): State<AppState>, Path(id): Path<Uuid>, Json(b): Json<CashBody>) -> Result<Json<Value>, ApiError> {
    let a = paper_account(&state, id).await?;
    let amount = b.amount;
    if amount.is_some_and(|x| x.is_sign_negative() && !x.is_zero()) {
        return Err(ApiError::bad_request("the amount is never negative"));
    }
    match b.kind.as_str() {
        "deposit" | "withdraw" => {
            let Some(x) = amount.filter(|x| !x.is_zero()) else {
                return Err(ApiError::bad_request("enter an amount more than zero"));
            };
            if b.kind == "withdraw" {
                let (free, currency) = paper::withdrawable(&state.pool, &a).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
                if x > free {
                    return Err(ApiError::bad_request(&format!(
                        "{:.2} {currency} can be withdrawn at most: the rest is not cash or backs the open positions' margin",
                        free
                    )));
                }
            }
            store::book_cash(&state.pool, id, &b.kind, x).await?;
        }
        "reset" => paper::reset(&state.pool, &a, amount).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?,
        _ => return Err(ApiError::bad_request("kind is deposit, withdraw or reset")),
    }
    let summary = paper::summary(&state.pool, &a).await.map_err(|e| ApiError::bad_request(&format!("{e:#}")))?;
    Ok(Json(json!({ "summary": summary })))
}
