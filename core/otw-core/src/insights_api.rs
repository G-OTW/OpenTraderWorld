//! HTTP API of the insights page: `POST /api/insights`.
//!
//! One source (the journal's closed trades, a recorded backtest run, or a paper session),
//! the researchers of [`crate::insights`] run over it, the findings returned with their
//! evidence. Reads only: nothing is written, nothing is fetched from a provider. A backtest
//! source replays the run, so the route sits behind the compute gate, and the answer is
//! kept in memory for a few minutes, keyed on a hash of the request and of what the source
//! reads (its trades, the fee schedules, the stored SIC codes): a change to any of them is
//! a new key.

use axum::{extract::State, routing::post, Extension, Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::insights::{self, sources, Insight, SourceKind, TradeSet, EFFECT_SHARE, MIN_SAMPLE};
use crate::journal_api::TradeQuery;
use crate::{ApiError, AppState};
use otw_store::journal::{self, TradeFilter};
use otw_store::journal_behavior::Insight as Statement;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/insights", post(run))
}

/// Which trades the researchers read. Exactly one.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SourceBody {
    /// The journal's closed trades matching the filters (same as /api/journal/trades;
    /// omitted = every closed trade). Needs the journal module.
    Journal {
        #[serde(default)]
        filters: Option<TradeQuery>,
    },
    /// One recorded backtest run (GET /api/backtest/runs), replayed. Needs the backtest module.
    Backtest { run_id: Uuid },
    /// One paper session's closed round trips (GET /api/paper/sessions). Needs the backtest module.
    Paper { session_id: Uuid },
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InsightsBody {
    pub source: SourceBody,
    /// Researchers to run (hours, weekday, hold_time, stop_geometry, concentration,
    /// instrument_fees, sizing_streaks, symmetry). Omitted = all of them.
    #[serde(default)]
    pub researchers: Option<Vec<String>>,
    /// Journal source: the viewer's offset in minutes, as `Date.getTimezoneOffset`, so
    /// hours and weekdays are local (-840 to 840). A run reads its own strategy clock.
    #[serde(default)]
    pub tz_offset: i32,
}

/// The body's own refusals, before anything is read. Nothing is clamped.
fn check(body: &InsightsBody) -> Result<(), String> {
    if let Some(r) = &body.researchers {
        if r.is_empty() {
            return Err("no researcher named: leave researchers out to run all of them".into());
        }
        for name in r {
            if !insights::known(name) {
                return Err(format!("unknown researcher '{name}': use one of {}", insights::names().join(", ")));
            }
        }
        let mut seen = r.clone();
        seen.sort();
        seen.dedup();
        if seen.len() != r.len() {
            return Err("a researcher is named twice: name each once".into());
        }
    }
    if !(-840..=840).contains(&body.tz_offset) {
        return Err(format!("tz_offset must be between -840 and 840 minutes (got {})", body.tz_offset));
    }
    Ok(())
}

/// The module a source reads, refused with the fix when it is not installed.
fn require(installed: &[String], kind: SourceKind) -> Result<(), ApiError> {
    let (module, name) = match kind {
        SourceKind::Journal => ("journal", "Trading Journal"),
        SourceKind::Backtest | SourceKind::Paper => ("backtest", "Backtest"),
    };
    if installed.iter().any(|m| m == module) {
        return Ok(());
    }
    Err(ApiError::bad_request_code(
        "module_missing",
        &format!("the {name} module is not installed: install it under Settings, Modules to read this source"),
    ))
}

fn kind_of(s: &SourceBody) -> SourceKind {
    match s {
        SourceBody::Journal { .. } => SourceKind::Journal,
        SourceBody::Backtest { .. } => SourceKind::Backtest,
        SourceBody::Paper { .. } => SourceKind::Paper,
    }
}

// ── Cache ────────────────────────────────────────────────────────────────────

const TTL: Duration = Duration::from_secs(300);
const CAP: usize = 64;

fn cache() -> &'static Mutex<HashMap<u64, (Instant, Value)>> {
    static C: OnceLock<Mutex<HashMap<u64, (Instant, Value)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(key: u64) -> Option<Value> {
    let c = cache().lock().ok()?;
    c.get(&key).filter(|(at, _)| at.elapsed() < TTL).map(|(_, v)| v.clone())
}

fn remember(key: u64, v: &Value) {
    let Ok(mut c) = cache().lock() else { return };
    c.retain(|_, (at, _)| at.elapsed() < TTL);
    while c.len() >= CAP {
        let Some(oldest) = c.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| *k) else { break };
        c.remove(&oldest);
    }
    c.insert(key, (Instant::now(), v.clone()));
}

pub(crate) fn key_of(raw: &Value, fingerprint: &str) -> u64 {
    let mut h = DefaultHasher::new();
    raw.to_string().hash(&mut h);
    fingerprint.hash(&mut h);
    h.finish()
}

/// What the answer depends on besides the request: the trades of the source, the fee
/// schedules, the stored SIC codes and the installed modules.
async fn fingerprint(state: &AppState, body: &InsightsBody, installed: &[String]) -> Result<String, ApiError> {
    let pool = &state.pool;
    let source = match &body.source {
        SourceBody::Journal { .. } => {
            let (n, at, m, mat): (i64, Option<OffsetDateTime>, i64, Option<OffsetDateTime>) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM journal_trades), (SELECT max(updated_at) FROM journal_trades), \
                        (SELECT count(*) FROM journal_trade_metrics), (SELECT max(computed_at) FROM journal_trade_metrics)",
            )
            .fetch_one(pool)
            .await
            .map_err(anyhow::Error::from)?;
            let currency = journal::get_settings(pool).await?.display_currency;
            format!("j:{n}:{at:?}:{m}:{mat:?}:{currency}")
        }
        SourceBody::Backtest { run_id } => {
            let run = otw_store::backtest::get_run(pool, *run_id).await?.ok_or_else(|| {
                ApiError::not_found("backtest run not found: it may have been cleared from the history, pick another run")
            })?;
            format!("b:{}:{}:{}", run.id, run.created_at, run.engine_version)
        }
        SourceBody::Paper { session_id } => {
            let (n, closed, opened): (i64, Option<OffsetDateTime>, Option<OffsetDateTime>) = sqlx::query_as(
                "SELECT count(*), max(closed_at), max(opened_at) FROM paper_trades WHERE session_id = $1",
            )
            .bind(session_id)
            .fetch_one(pool)
            .await
            .map_err(anyhow::Error::from)?;
            format!("p:{session_id}:{n}:{closed:?}:{opened:?}")
        }
    };
    let schedules = if installed.iter().any(|m| m == "journal") {
        format!("{:?}", journal::list_fee_schedules(pool).await?)
    } else {
        String::new()
    };
    let (sic,): (i64,) = sqlx::query_as("SELECT count(*) FROM fund_companies WHERE sic <> ''")
        .fetch_one(pool)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(format!("{source}|{schedules}|{sic}|{}", installed.join(",")))
}

// ── Handler ──────────────────────────────────────────────────────────────────

/// Run the researchers over one source. Reads only.
async fn run(
    State(state): State<AppState>,
    ip: Option<Extension<crate::demo::ClientIp>>,
    Json(raw): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let body: InsightsBody = serde_json::from_value(raw.clone())
        .map_err(|e| ApiError::bad_request(&format!("insights body: {e}")))?;
    check(&body).map_err(|e| ApiError::bad_request(&e))?;
    let installed = otw_store::data_admin::installed_modules(&state.pool).await?;
    require(&installed, kind_of(&body.source))?;

    let key = key_of(&raw, &fingerprint(&state, &body, &installed).await?);
    if let Some(mut v) = cached(key) {
        v["cached"] = json!(true);
        return Ok(Json(v));
    }

    let pool = &state.pool;
    let InsightsBody { source, researchers, tz_offset } = body;
    let mut behavior: Vec<Statement> = Vec::new();
    let mut exposure: Vec<Statement> = Vec::new();
    let (set, about): (TradeSet, Value) = match source {
        SourceBody::Journal { filters } => {
            let filter = match filters {
                Some(q) => q.into_filter()?,
                None => TradeFilter::default(),
            };
            let currency = journal::get_settings(pool).await?.display_currency;
            let set = sources::load_journal(pool, &filter, &currency, tz_offset).await?;
            // The journal's own statements, listed unchanged next to the researchers'.
            behavior = otw_store::journal_analytics::analytics(pool, &filter, &currency, tz_offset)
                .await?
                .behavior
                .insights;
            exposure = crate::journal_market::exposure(pool, &filter, &currency).await?.warnings;
            (set, json!({ "kind": "journal", "category_id": filter.category_id, "strategy_id": filter.strategy_id }))
        }
        SourceBody::Backtest { run_id } => {
            crate::backtest_api::demo_sim_allowed(ip)?;
            let (set, run) = sources::load_backtest(&state, run_id).await?;
            (
                set,
                json!({ "kind": "backtest", "run_id": run.id, "name": run.name, "ticker": run.ticker,
                        "timeframe": run.timeframe, "strategy_id": run.strategy_id }),
            )
        }
        SourceBody::Paper { session_id } => {
            let session = otw_store::paper::get_session(pool, session_id).await?.ok_or_else(|| {
                ApiError::not_found("paper session not found: it may have been deleted, pick another session")
            })?;
            let set = sources::load_paper(pool, &session).await?;
            (
                set,
                json!({ "kind": "paper", "session_id": session.id, "name": session.name,
                        "strategy_id": session.strategy_id }),
            )
        }
    };

    let ctx = sources::context(pool, &set, installed.iter().any(|m| m == "journal")).await?;
    let found: Vec<Insight> = insights::research(&set, &ctx, researchers.as_deref());
    let ran: Vec<&str> = match &researchers {
        Some(r) => insights::names().into_iter().filter(|n| r.iter().any(|x| x == n)).collect(),
        None => insights::names(),
    };
    let out = json!({
        "source": about,
        "currency": set.currency,
        "sample": set.trades.len(),
        "unmeasured": set.unmeasured.len(),
        "min_sample": MIN_SAMPLE,
        "effect_share": EFFECT_SHARE,
        "researchers": ran,
        "fee_schedules": ctx.schedules.len(),
        "journal_installed": ctx.journal_installed,
        "insights": found,
        "behavior": behavior,
        "exposure": exposure,
        "cached": false,
    });
    remember(key, &out);
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(v: Value) -> InsightsBody {
        serde_json::from_value(v).expect("valid body")
    }

    #[test]
    fn routes_build() {
        let _ = routes();
    }

    #[test]
    fn a_body_names_exactly_one_known_source() {
        assert!(serde_json::from_value::<InsightsBody>(json!({ "source": { "excel": {} } })).is_err());
        assert!(serde_json::from_value::<InsightsBody>(json!({})).is_err());
        assert!(serde_json::from_value::<InsightsBody>(json!({ "source": { "backtest": {} } })).is_err());
        assert!(
            serde_json::from_value::<InsightsBody>(json!({ "source": { "backtest": { "run_id": Uuid::nil(), "x": 1 } } }))
                .is_err()
        );
        let b = body(json!({ "source": { "journal": { "filters": { "category_id": Uuid::nil() } } } }));
        assert!(matches!(b.source, SourceBody::Journal { filters: Some(_) }));
        let b = body(json!({ "source": { "journal": {} } }));
        assert!(matches!(b.source, SourceBody::Journal { filters: None }));
        let b = body(json!({ "source": { "paper": { "session_id": Uuid::nil() } } }));
        assert_eq!(kind_of(&b.source), SourceKind::Paper);
        assert!(check(&b).is_ok());
    }

    #[test]
    fn researchers_are_checked_by_name_and_never_clamped() {
        let src = json!({ "backtest": { "run_id": Uuid::nil() } });
        let b = body(json!({ "source": src, "researchers": [] }));
        assert!(check(&b).unwrap_err().contains("no researcher"));
        let b = body(json!({ "source": src, "researchers": ["astrology"] }));
        assert!(check(&b).unwrap_err().contains("unknown researcher 'astrology'"));
        let b = body(json!({ "source": src, "researchers": ["hours", "hours"] }));
        assert!(check(&b).unwrap_err().contains("twice"));
        let b = body(json!({ "source": src, "researchers": ["hours", "symmetry"] }));
        assert!(check(&b).is_ok());
        let b = body(json!({ "source": src, "tz_offset": 900 }));
        assert!(check(&b).unwrap_err().contains("tz_offset"));
    }

    #[test]
    fn a_source_needs_its_module_and_says_which() {
        let only_journal = vec!["journal".to_string()];
        assert!(require(&only_journal, SourceKind::Journal).is_ok());
        let e = require(&only_journal, SourceKind::Backtest).unwrap_err();
        assert_eq!(e.status_code(), axum::http::StatusCode::BAD_REQUEST);
        assert!(e.message().contains("Backtest module is not installed"));
        assert!(require(&[], SourceKind::Paper).is_err());
    }

    #[test]
    fn the_cache_key_follows_the_request_and_the_source() {
        let a = json!({ "source": { "journal": {} } });
        let b = json!({ "source": { "journal": {} }, "researchers": ["hours"] });
        assert_eq!(key_of(&a, "j:1"), key_of(&a, "j:1"));
        assert_ne!(key_of(&a, "j:1"), key_of(&a, "j:2"));
        assert_ne!(key_of(&a, "j:1"), key_of(&b, "j:1"));
        remember(key_of(&a, "test"), &json!({ "ok": true }));
        assert_eq!(cached(key_of(&a, "test")), Some(json!({ "ok": true })));
        assert_eq!(cached(key_of(&a, "other")), None);
    }
}
