//! Scanner API: items, hives, signals, alerts, hits, boards and their views.
//!
//! - `GET/PUT          /api/scanner/settings`            the hive cap
//! - `GET/POST         /api/scanner/items`                the universe; `PUT/DELETE …/{id}`
//! - `GET/POST         /api/scanner/hives`                groups of items; `PUT/DELETE …/{id}`
//! - `PUT              /api/scanner/hives/{id}/items`     replace a hive's members (capped)
//! - `GET              /api/scanner/hives/{id}/board`     the hive's items with their cache
//! - `GET/POST         /api/scanner/signals`              reusable signals; `PUT/DELETE …/{id}`
//! - `POST             /api/scanner/signals/test`         judge a draft on one item's stored bars
//! - `POST             /api/scanner/signals/{id}/test`    the same for a saved signal
//! - `GET/POST         /api/scanner/alerts`               `PUT/DELETE …/{id}`
//! - `POST             /api/scanner/alerts/{id}/run-now`  bounded preview, records nothing
//! - `GET              /api/scanner/hits`                 the hit log
//! - `GET/POST         /api/scanner/boards`               named sets of views; `PUT/DELETE …/{id}`
//! - `GET/POST         /api/scanner/views`                board views; `PUT/DELETE …/{id}`
//!
//! The scanner only notifies: nothing here places an order or sends anywhere but the
//! user's own notification channels, and only from the background job.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::histdata as hist_store;
use otw_store::scanner::{self as store, ItemInput};

use crate::scanner::eval::{self, Compiled};
use crate::scanner::Ohlcv;
use crate::scanner_job::{self as job, ScanCx};
use crate::{ApiError, AppState};

/// Stored bars a signal test reads at most.
const TEST_BARS: i64 = 2000;
/// Held bars a test answers with at most, newest kept.
const TEST_HELD: usize = 300;
const HITS_LIMIT: i64 = 500;
const METRICS: &[&str] = &["change_pct", "change_from_open", "volume_ratio", "hit_recency"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/scanner/settings", get(get_settings).put(put_settings))
        .route("/api/scanner/items", get(list_items).post(create_item))
        .route("/api/scanner/items/{id}", put(update_item).delete(delete_item))
        .route("/api/scanner/hives", get(list_hives).post(create_hive))
        .route("/api/scanner/hives/{id}", put(update_hive).delete(delete_hive))
        .route("/api/scanner/hives/{id}/items", put(set_hive_items))
        .route("/api/scanner/hives/{id}/board", get(board))
        .route("/api/scanner/signals", get(list_signals).post(create_signal))
        .route("/api/scanner/signals/test", post(test_draft))
        .route("/api/scanner/signals/{id}", put(update_signal).delete(delete_signal))
        .route("/api/scanner/signals/{id}/test", post(test_signal))
        .route("/api/scanner/alerts", get(list_alerts).post(create_alert))
        .route("/api/scanner/alerts/{id}", put(update_alert).delete(delete_alert))
        .route("/api/scanner/alerts/{id}/run-now", post(run_now))
        .route("/api/scanner/hits", get(list_hits))
        .route("/api/scanner/boards", get(list_boards).post(create_board))
        .route("/api/scanner/boards/{id}", put(update_board).delete(delete_board))
        .route("/api/scanner/views", get(list_views).post(create_view))
        .route("/api/scanner/views/{id}", put(update_view).delete(delete_view))
}

fn cx(state: &AppState) -> ScanCx {
    ScanCx { cipher: state.cipher.clone(), http: state.http.clone() }
}

/// An assigned color: empty (inherit), a chart palette slot (`chart-1` to `chart-8`, which
/// follows the light and dark themes), or a hex color.
fn color(v: Option<&str>) -> Result<String, ApiError> {
    let c = v.unwrap_or("").trim();
    let hex = c.strip_prefix('#').unwrap_or("");
    let slot = c.strip_prefix("chart-").and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=8).contains(&n));
    if c.is_empty() || slot || ([3, 6, 8].contains(&hex.len()) && hex.chars().all(|ch| ch.is_ascii_hexdigit())) {
        Ok(c.to_string())
    } else {
        Err(ApiError::bad_request("a color is a palette slot like chart-3 or a hex value like #3b82f6"))
    }
}

fn required<'a>(name: &str, v: &'a str) -> Result<&'a str, ApiError> {
    let v = v.trim();
    if v.is_empty() {
        return Err(ApiError::bad_request(&format!("{name} is required")));
    }
    Ok(v)
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SettingsBody {
    /// Items a hive may hold, 1 to 2000. Default 200.
    hive_cap: i64,
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let cap: i64 = store::hive_cap(&state.pool).await?;
    Ok(Json(json!({ "hive_cap": cap, "default_hive_cap": store::DEFAULT_HIVE_CAP, "max_hive_cap": store::MAX_HIVE_CAP })))
}

async fn put_settings(State(state): State<AppState>, Json(b): Json<SettingsBody>) -> Result<Json<Value>, ApiError> {
    if !(1..=store::MAX_HIVE_CAP).contains(&b.hive_cap) {
        return Err(ApiError::bad_request(&format!("the hive cap goes from 1 to {}", store::MAX_HIVE_CAP)));
    }
    store::set_hive_cap(&state.pool, b.hive_cap).await?;
    Ok(Json(json!({ "hive_cap": b.hive_cap })))
}

// ── Items ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ItemBody {
    /// Provider id, as the symbol lookup returned it.
    provider: String,
    asset_type: String,
    /// The provider's own symbol, as the symbol lookup returned it. Never guessed.
    ticker: String,
    timeframe: String,
    /// The data connector to read through; absent = the oldest one granted to the scanner.
    connector_id: Option<Uuid>,
    label: Option<String>,
    /// Hex color; empty = the hive's color.
    color: Option<String>,
    /// Hives to add the new item to (create only).
    #[serde(default)]
    hive_ids: Vec<Uuid>,
}

impl ItemBody {
    async fn check(&self, state: &AppState) -> Result<String, ApiError> {
        let provider = required("provider", &self.provider)?;
        let asset_type = required("asset_type", &self.asset_type)?;
        required("ticker", &self.ticker)?;
        let timeframe = required("timeframe", &self.timeframe)?;
        crate::histdata::validate_request(provider, asset_type, timeframe)
            .map_err(|e| ApiError::bad_request(&e.to_string()))?;
        if let Some(id) = self.connector_id {
            let c = otw_store::connectors::get(&state.pool, id)
                .await?
                .ok_or_else(|| ApiError::bad_request("unknown connector"))?;
            if c.provider != provider {
                return Err(ApiError::bad_request(&format!("the connector {} is a {} account, not {provider}", c.name, c.provider)));
            }
            if !c.allows(job::MODULE) {
                return Err(ApiError::bad_request(&format!(
                    "the connector {} is not granted to the Scanner: grant it in the data broker",
                    c.name
                )));
            }
        }
        color(self.color.as_deref())
    }

    fn input<'a>(&'a self, color: &'a str) -> ItemInput<'a> {
        ItemInput {
            provider: self.provider.trim(),
            asset_type: self.asset_type.trim(),
            ticker: self.ticker.trim(),
            timeframe: self.timeframe.trim(),
            connector_id: self.connector_id,
            label: self.label.as_deref().unwrap_or("").trim(),
            color,
        }
    }
}

async fn list_items(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let items = store::list_items(&state.pool).await?;
    Ok(Json(json!({ "items": items })))
}

async fn create_item(State(state): State<AppState>, Json(b): Json<ItemBody>) -> Result<Json<Value>, ApiError> {
    let c = b.check(&state).await?;
    let input = b.input(&c);
    if let Some(dup) = store::find_item(&state.pool, input.provider, input.ticker, input.timeframe, input.connector_id).await? {
        return Err(ApiError::conflict(&format!(
            "{} {} on this connector is already an item",
            dup.ticker, dup.timeframe
        )));
    }
    // The demo stores nothing it would have to download: an item it cannot read from the
    // seeded catalog would have the scan fetch it every bar.
    if crate::demo::enabled()
        && hist_store::find_dataset(&state.pool, input.provider, input.asset_type, input.ticker, input.timeframe)
            .await?
            .is_none()
    {
        return Err(ApiError::forbidden(
            "The demo scans the stored instruments only: adding one that needs a download is off.",
        ));
    }
    // Hives first, so a cap refusal leaves no orphan item behind.
    let cap = store::hive_cap(&state.pool).await?;
    let mut hives = Vec::new();
    for hid in &b.hive_ids {
        let h = store::get_hive(&state.pool, *hid).await?.ok_or_else(|| ApiError::not_found("hive"))?;
        if let Some(why) = store::cap_refusal(h.item_ids.len() as i64 + 1, cap) {
            return Err(ApiError::bad_request(&format!("{}: {why}", h.name)));
        }
        hives.push(h);
    }
    let item = store::create_item(&state.pool, input).await?;
    for h in hives {
        let mut ids = h.item_ids.clone();
        ids.push(item.id);
        store::set_hive_items(&state.pool, h.id, &ids).await?;
    }
    let item = store::get_item(&state.pool, item.id).await?.ok_or_else(|| ApiError::not_found("item"))?;
    Ok(Json(json!(item)))
}

async fn update_item(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ItemBody>,
) -> Result<Json<Value>, ApiError> {
    let c = b.check(&state).await?;
    let input = b.input(&c);
    if let Some(dup) = store::find_item(&state.pool, input.provider, input.ticker, input.timeframe, input.connector_id).await? {
        if dup.id != id {
            return Err(ApiError::conflict(&format!("{} {} on this connector is already an item", dup.ticker, dup.timeframe)));
        }
    }
    let item = store::update_item(&state.pool, id, input).await?.ok_or_else(|| ApiError::not_found("item"))?;
    Ok(Json(json!(item)))
}

async fn delete_item(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete_item(&state.pool, id).await? {
        return Err(ApiError::not_found("item"));
    }
    Ok(Json(json!({ "deleted": true })))
}

// ── Hives ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct HiveBody {
    name: String,
    color: Option<String>,
    sort: Option<i32>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct HiveItemsBody {
    /// Every member, replacing the current list.
    item_ids: Vec<Uuid>,
}

async fn list_hives(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let cap = store::hive_cap(&state.pool).await?;
    let hives = store::list_hives(&state.pool).await?;
    Ok(Json(json!({ "hives": hives, "cap": cap })))
}

async fn create_hive(State(state): State<AppState>, Json(b): Json<HiveBody>) -> Result<Json<Value>, ApiError> {
    let name = required("name", &b.name)?;
    let c = color(b.color.as_deref())?;
    let h = store::create_hive(&state.pool, name, &c).await?;
    Ok(Json(json!(h)))
}

async fn update_hive(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<HiveBody>,
) -> Result<Json<Value>, ApiError> {
    let name = required("name", &b.name)?;
    let c = color(b.color.as_deref())?;
    let h = store::update_hive(&state.pool, id, name, &c, b.sort).await?.ok_or_else(|| ApiError::not_found("hive"))?;
    Ok(Json(json!(h)))
}

async fn delete_hive(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete_hive(&state.pool, id).await? {
        return Err(ApiError::not_found("hive"));
    }
    Ok(Json(json!({ "deleted": true })))
}

async fn set_hive_items(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<HiveItemsBody>,
) -> Result<Json<Value>, ApiError> {
    let h = store::get_hive(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("hive"))?;
    let mut ids = b.item_ids.clone();
    ids.sort();
    ids.dedup();
    let cap = store::hive_cap(&state.pool).await?;
    if let Some(why) = store::cap_refusal(ids.len() as i64, cap) {
        return Err(ApiError::bad_request(&why));
    }
    let known = store::items_by_ids(&state.pool, &ids).await?;
    if known.len() != ids.len() {
        return Err(ApiError::bad_request(&format!("{} of these items no longer exist", ids.len() - known.len())));
    }
    store::set_hive_items(&state.pool, h.id, &ids).await?;
    let h = store::get_hive(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("hive"))?;
    Ok(Json(json!(h)))
}

/// A hive's items with their cache and last hit: what every board view draws.
async fn board(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let h = store::get_hive(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("hive"))?;
    let items = store::items_by_ids(&state.pool, &h.item_ids).await?;
    let last: std::collections::HashMap<Uuid, OffsetDateTime> =
        store::last_hits(&state.pool, &h.item_ids).await?.into_iter().collect();
    let rows: Vec<Value> = items
        .into_iter()
        .map(|it| {
            let mut v = json!(it);
            let at = last.get(&it.id).and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok());
            v["last_hit_at"] = json!(at);
            v
        })
        .collect();
    Ok(Json(json!({ "hive": h, "items": rows })))
}

// ── Signals ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SignalBody {
    name: String,
    /// `signal` (a backtest signal group: {logic: all|any, conditions: [{left, op, right}]}),
    /// `event` ({kind: move_from_open|move|new_high|new_low|gap|volume_spike|range_expansion,
    /// ...}), `pattern` ({kind: engulfing|pin_bar|inside_bar|outside_bar|doji|hammer|
    /// shooting_star|three_bar_reversal|breakout|consolidation|hh_hl|lh_ll, ...}) or `time`
    /// ({sessions, weekdays, from, to, timezone, at}).
    builder: String,
    /// The shape follows `builder`; published so a typo in an optional key shows up in the
    /// MCP validation instead of being dropped.
    #[schemars(with = "SignalConfigSchema")]
    config: Value,
    color: Option<String>,
}

/// What a signal's `config` holds, one shape per builder. Schema only: the handler reads
/// the config with `eval::parse`, which picks the shape from `builder`.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
pub(crate) enum SignalConfigSchema {
    /// builder=signal: a backtest signal group.
    Signal(crate::backtest::SignalGroup),
    /// builder=event: tagged by `kind`.
    Event(crate::scanner::events::Event),
    /// builder=pattern: tagged by `kind`.
    Pattern(crate::scanner::patterns::Pattern),
    /// builder=time: a session, weekday and hour window.
    Time(crate::scanner::time::TimeSpec),
}

/// Parse and resolve a signal against the library, or say why it cannot be judged.
async fn compile(state: &AppState, id: Uuid, name: &str, builder: &str, config: &Value) -> Result<Compiled, ApiError> {
    let spec = eval::parse(builder, config).map_err(|e| ApiError::bad_request(&e))?;
    let lib = job::library_for(&state.pool, std::iter::once(&spec)).await?;
    Compiled::new(id, name, spec, &lib).map_err(|e| ApiError::bad_request(&e))
}

async fn list_signals(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = store::list_signals(&state.pool).await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|s| {
            let summary = eval::parse(&s.builder, &s.config).map(|p| p.describe());
            let mut v = json!(s);
            match summary {
                Ok(d) => v["summary"] = json!(d),
                Err(e) => v["error"] = json!(e),
            }
            v
        })
        .collect();
    Ok(Json(json!({ "signals": out })))
}

async fn create_signal(State(state): State<AppState>, Json(b): Json<SignalBody>) -> Result<Json<Value>, ApiError> {
    let name = required("name", &b.name)?;
    let c = color(b.color.as_deref())?;
    compile(&state, Uuid::nil(), name, &b.builder, &b.config).await?;
    let s = store::create_signal(&state.pool, name, &b.builder, &b.config, &c).await?;
    Ok(Json(json!(s)))
}

async fn update_signal(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<SignalBody>,
) -> Result<Json<Value>, ApiError> {
    let name = required("name", &b.name)?;
    let c = color(b.color.as_deref())?;
    compile(&state, id, name, &b.builder, &b.config).await?;
    let s = store::update_signal(&state.pool, id, name, &b.builder, &b.config, &c)
        .await?
        .ok_or_else(|| ApiError::not_found("signal"))?;
    Ok(Json(json!(s)))
}

async fn delete_signal(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    let users = store::alerts_using_signal(&state.pool, id).await?;
    if !users.is_empty() {
        return Err(ApiError::conflict(&format!(
            "This signal is used by {}: remove it from them first.",
            users.join(", ")
        )));
    }
    if !store::delete_signal(&state.pool, id).await? {
        return Err(ApiError::not_found("signal"));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TestBody {
    item_id: Uuid,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct DraftTestBody {
    item_id: Uuid,
    builder: String,
    config: Value,
}

async fn test_signal(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<TestBody>,
) -> Result<Json<Value>, ApiError> {
    let s = store::get_signal(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("signal"))?;
    let c = compile(&state, s.id, &s.name, &s.builder, &s.config).await?;
    run_test(&state, b.item_id, &c).await
}

async fn test_draft(State(state): State<AppState>, Json(b): Json<DraftTestBody>) -> Result<Json<Value>, ApiError> {
    let c = compile(&state, Uuid::nil(), "draft", &b.builder, &b.config).await?;
    run_test(&state, b.item_id, &c).await
}

/// Judge a signal on the stored bars of one item: where it held, newest last. Reads the
/// catalog only, so it costs no request.
async fn run_test(state: &AppState, item_id: Uuid, c: &Compiled) -> Result<Json<Value>, ApiError> {
    let it = store::get_item(&state.pool, item_id).await?.ok_or_else(|| ApiError::not_found("item"))?;
    let tf = crate::histdata::timeframe_secs(&it.timeframe).map_err(|e| ApiError::bad_request(&e.to_string()))?;
    let Some(d) = hist_store::find_dataset(&state.pool, &it.provider, &it.asset_type, &it.ticker, &it.timeframe).await? else {
        return Err(ApiError::bad_request(&format!(
            "{} {} has no stored bars: download it in Historical Data first, or use Run now on an alert",
            it.ticker, it.timeframe
        )));
    };
    let bars = hist_store::read_bars(&state.pool, d.id, None, None, TEST_BARS).await?;
    let closed = crate::histviz_alerts::closed_bars(&bars, tf, OffsetDateTime::now_utc());
    let s = Ohlcv::of(&closed);
    let held = if c.reads_script() {
        let h = eval::script_history(job::syminfo(&state.pool, &it).await?, &it.timeframe);
        crate::script::watched(|| eval::held_bars(&[c], &s, tf, Some(&h))).await
    } else {
        eval::held_bars(&[c], &s, tf, None)
    };
    let total = held.len();
    let fmt = |t: OffsetDateTime| t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let list: Vec<String> = held.iter().rev().take(TEST_HELD).rev().map(|&i| fmt(s.ts[i])).collect();
    Ok(Json(json!({
        "item_id": it.id,
        "ticker": it.ticker,
        "timeframe": it.timeframe,
        "bars": s.len(),
        "from": s.ts.first().map(|t| fmt(*t)),
        "to": s.ts.last().map(|t| fmt(*t)),
        "held_count": total,
        "held": list,
    })))
}

// ── Alerts ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AlertBody {
    name: Option<String>,
    enabled: Option<bool>,
    /// `item` or `hive`.
    target_kind: String,
    target_id: Uuid,
    /// Every one must hold on the same closed bar. OR is two alerts.
    signal_ids: Vec<Uuid>,
    /// `digest` (one notification per run listing the hits, default) or `per_hit`.
    notify: Option<String>,
    /// Notification channels; empty = every channel the scanner is granted.
    #[serde(default)]
    channels: Vec<Uuid>,
    /// No second hit on the same item within this many seconds. 0 = none.
    cooldown_secs: Option<i32>,
}

impl AlertBody {
    async fn check(&self, state: &AppState) -> Result<(), ApiError> {
        match self.target_kind.as_str() {
            "item" => {
                store::get_item(&state.pool, self.target_id).await?.ok_or_else(|| ApiError::bad_request("unknown item"))?;
            }
            "hive" => {
                store::get_hive(&state.pool, self.target_id).await?.ok_or_else(|| ApiError::bad_request("unknown hive"))?;
            }
            _ => return Err(ApiError::bad_request("target_kind must be item or hive")),
        }
        if self.signal_ids.is_empty() {
            return Err(ApiError::bad_request("pick at least one signal"));
        }
        let found = store::signals_by_ids(&state.pool, &self.signal_ids).await?;
        if found.len() != dedup(&self.signal_ids).len() {
            return Err(ApiError::bad_request("one of these signals does not exist"));
        }
        if !["digest", "per_hit"].contains(&self.notify.as_deref().unwrap_or("digest")) {
            return Err(ApiError::bad_request("notify must be digest or per_hit"));
        }
        if self.cooldown_secs.unwrap_or(0) < 0 {
            return Err(ApiError::bad_request("the cooldown cannot be negative"));
        }
        for id in &self.channels {
            match otw_store::notif_channels::get(&state.pool, *id).await? {
                Some(ch) if ch.allows(job::MODULE) => {}
                Some(_) => return Err(ApiError::bad_request("this notification channel is not allowed for the Scanner")),
                None => return Err(ApiError::bad_request("unknown notification channel")),
            }
        }
        Ok(())
    }
}

fn dedup(ids: &[Uuid]) -> Vec<Uuid> {
    let mut v = ids.to_vec();
    v.sort();
    v.dedup();
    v
}

async fn list_alerts(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let alerts = store::list_alerts(&state.pool).await?;
    Ok(Json(json!({ "alerts": alerts })))
}

async fn save_alert(state: &AppState, id: Option<Uuid>, b: AlertBody) -> Result<Json<Value>, ApiError> {
    b.check(state).await?;
    let channels = json!(b.channels);
    let ids = dedup(&b.signal_ids);
    let input = store::AlertInput {
        name: b.name.as_deref().unwrap_or("").trim(),
        enabled: b.enabled.unwrap_or(true),
        target_kind: &b.target_kind,
        target_id: b.target_id,
        signal_ids: &ids,
        notify: b.notify.as_deref().unwrap_or("digest"),
        channels: &channels,
        cooldown_secs: b.cooldown_secs.unwrap_or(0),
    };
    let a = match id {
        None => store::create_alert(&state.pool, input).await?,
        Some(id) => store::update_alert(&state.pool, id, input).await?.ok_or_else(|| ApiError::not_found("alert"))?,
    };
    Ok(Json(json!(a)))
}

async fn create_alert(State(state): State<AppState>, Json(b): Json<AlertBody>) -> Result<Json<Value>, ApiError> {
    save_alert(&state, None, b).await
}

async fn update_alert(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<AlertBody>,
) -> Result<Json<Value>, ApiError> {
    save_alert(&state, Some(id), b).await
}

async fn delete_alert(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete_alert(&state.pool, id).await? {
        return Err(ApiError::not_found("alert"));
    }
    Ok(Json(json!({ "deleted": true })))
}

/// Judge the alert on its target now: a preview that records no hit and notifies nobody.
async fn run_now(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if crate::demo::enabled() {
        return Err(ApiError::forbidden("Running a scan now is off in the demo."));
    }
    let a = store::get_alert(&state.pool, id).await?.ok_or_else(|| ApiError::not_found("alert"))?;
    let cap = store::hive_cap(&state.pool).await?;
    let items = job::target_items(&state.pool, &a, cap).await?.map_err(|e| ApiError::bad_request(&e))?;
    let compiled = job::compile_all(&state.pool, a.signal_ids.iter().copied()).await?;
    let mut sigs = Vec::new();
    for sid in &a.signal_ids {
        match compiled.get(sid) {
            Some(Ok(c)) => sigs.push(c),
            Some(Err(e)) => return Err(ApiError::bad_request(e)),
            None => return Err(ApiError::bad_request("A signal of this alert was deleted: pick another one.")),
        }
    }
    let rows = job::preview(&state.pool, &cx(&state), items, &sigs).await;
    let hits = rows.iter().filter(|r| r.held).count();
    Ok(Json(json!({ "alert_id": a.id, "hits": hits, "rows": rows })))
}

// ── Hits ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct HitsQuery {
    alert_id: Option<Uuid>,
    item_id: Option<Uuid>,
    hive_id: Option<Uuid>,
    /// Newest first, at most 500 (default 200).
    limit: Option<i64>,
}

async fn list_hits(State(state): State<AppState>, Query(q): Query<HitsQuery>) -> Result<Json<Value>, ApiError> {
    let f = store::HitFilter {
        alert_id: q.alert_id,
        item_id: q.item_id,
        hive_id: q.hive_id,
        limit: q.limit.unwrap_or(200).clamp(1, HITS_LIMIT),
    };
    let hits = store::list_hits(&state.pool, &f).await?;
    Ok(Json(json!({ "hits": hits })))
}

// ── Boards ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct BoardBody {
    name: String,
    sort: Option<i32>,
}

async fn list_boards(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let boards = store::list_boards(&state.pool).await?;
    Ok(Json(json!({ "boards": boards })))
}

async fn create_board(State(state): State<AppState>, Json(b): Json<BoardBody>) -> Result<Json<Value>, ApiError> {
    required("name", &b.name)?;
    let board = store::create_board(&state.pool, b.name.trim(), b.sort).await?;
    Ok(Json(json!(board)))
}

async fn update_board(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<BoardBody>,
) -> Result<Json<Value>, ApiError> {
    required("name", &b.name)?;
    let board = store::update_board(&state.pool, id, b.name.trim(), b.sort)
        .await?
        .ok_or_else(|| ApiError::not_found("board"))?;
    Ok(Json(json!(board)))
}

/// Deletes the board with its views; the hives and items stay.
async fn delete_board(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete_board(&state.pool, id).await? {
        return Err(ApiError::not_found("board"));
    }
    Ok(Json(json!({ "deleted": true })))
}

// ── Views ────────────────────────────────────────────────────────────────────

async fn board_known(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    if store::list_boards(&state.pool).await?.iter().any(|b| b.id == id) {
        Ok(())
    } else {
        Err(ApiError::bad_request("board_id names no board"))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ViewBody {
    /// The board the view sits on.
    board_id: Uuid,
    name: String,
    /// `grid`, `honeycomb` or `lines`.
    kind: String,
    hive_id: Option<Uuid>,
    /// `{metric: change_pct|change_from_open|volume_ratio|hit_recency|signal:<id>}`.
    color_by: Option<Value>,
    /// The client's own layout (size, live switch, options).
    layout: Option<Value>,
    sort: Option<i32>,
}

impl ViewBody {
    fn check(&self) -> Result<(Value, Value), ApiError> {
        required("name", &self.name)?;
        if !["grid", "honeycomb", "lines"].contains(&self.kind.as_str()) {
            return Err(ApiError::bad_request("kind must be grid, honeycomb or lines"));
        }
        let color_by = self.color_by.clone().unwrap_or_else(|| json!({ "metric": "change_pct" }));
        let metric = color_by.get("metric").and_then(Value::as_str).unwrap_or("");
        let ok = METRICS.contains(&metric)
            || metric.strip_prefix("signal:").is_some_and(|s| Uuid::parse_str(s).is_ok());
        if !ok {
            return Err(ApiError::bad_request(
                "color_by.metric must be change_pct, change_from_open, volume_ratio, hit_recency or signal:<id>",
            ));
        }
        let layout = self.layout.clone().unwrap_or_else(|| json!({}));
        if !layout.is_object() {
            return Err(ApiError::bad_request("layout must be an object"));
        }
        Ok((color_by, layout))
    }
}

async fn list_views(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let views = store::list_views(&state.pool).await?;
    Ok(Json(json!({ "views": views })))
}

async fn create_view(State(state): State<AppState>, Json(b): Json<ViewBody>) -> Result<Json<Value>, ApiError> {
    let (color_by, layout) = b.check()?;
    board_known(&state, b.board_id).await?;
    let v = store::create_view(
        &state.pool,
        store::ViewInput { board_id: b.board_id, name: b.name.trim(), kind: &b.kind, hive_id: b.hive_id, color_by: &color_by, layout: &layout, sort: b.sort },
    )
    .await?;
    Ok(Json(json!(v)))
}

async fn update_view(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(b): Json<ViewBody>,
) -> Result<Json<Value>, ApiError> {
    let (color_by, layout) = b.check()?;
    board_known(&state, b.board_id).await?;
    let v = store::update_view(
        &state.pool,
        id,
        store::ViewInput { board_id: b.board_id, name: b.name.trim(), kind: &b.kind, hive_id: b.hive_id, color_by: &color_by, layout: &layout, sort: b.sort },
    )
    .await?
    .ok_or_else(|| ApiError::not_found("view"))?;
    Ok(Json(json!(v)))
}

async fn delete_view(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    if !store::delete_view(&state.pool, id).await? {
        return Err(ApiError::not_found("view"));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_api_colors_are_hex_or_empty() {
        assert_eq!(color(None).ok().as_deref(), Some(""));
        assert_eq!(color(Some("#3b82f6")).ok().as_deref(), Some("#3b82f6"));
        assert!(color(Some("chart-3")).is_ok());
        assert!(color(Some("chart-9")).is_err());
        assert!(color(Some("blue")).is_err());
        assert!(color(Some("#12345")).is_err());
    }

    #[test]
    fn scanner_api_view_metric_is_checked() {
        let v = |m: &str| ViewBody {
            board_id: Uuid::nil(),
            name: "V".into(),
            kind: "honeycomb".into(),
            hive_id: None,
            color_by: Some(json!({ "metric": m })),
            layout: None,
            sort: None,
        };
        assert!(v("change_pct").check().is_ok());
        assert!(v(&format!("signal:{}", Uuid::nil())).check().is_ok());
        assert!(v("price").check().is_err());
    }

    /// `config` publishes its shapes, so the MCP validation can see inside it.
    #[test]
    fn scanner_api_signal_config_has_a_schema() {
        let s = crate::mcp::catalog::schema::<SignalBody>().to_string();
        for key in ["conditions", "logic", "volume_spike", "engulfing", "weekdays", "sessions"] {
            assert!(s.contains(key), "{key} missing from {s}");
        }
    }
}
