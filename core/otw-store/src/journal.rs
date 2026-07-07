//! Storage + analytics for the Trading Journal module.
//!
//! Trades carry typed reserved columns (side, prices, quantity, fees, leverage,
//! multiplier) so PnL and performance stats can be computed for every trade
//! regardless of which template logged it. Custom template fields live in
//! `journal_trades.fields` (JSONB). Categories group trades; per-category capital
//! events (beginning stack + refills) anchor the equity curve and return metrics.
//!
//! Single-user: no owner scoping (consistent with documents/databases/feeds).

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::types::JsonValue;
use sqlx::AssertSqlSafe;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Deserialize a present field into `Some(value)` (value may itself be null → `Some(None)`),
/// so a patch can tell "field omitted" (`None`) from "field set to null" (`Some(None)`).
fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

// ── Categories ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Category {
    pub id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub description: Option<String>,
    pub position: f64,
    pub is_default: bool,
}

pub async fn list_categories(pool: &PgPool) -> anyhow::Result<Vec<Category>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, Option<String>, f64, bool)>(
        "SELECT id, name, color, description, position, is_default \
         FROM journal_categories ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing journal categories")?;
    Ok(rows
        .into_iter()
        .map(|(id, name, color, description, position, is_default)| Category {
            id,
            name,
            color,
            description,
            position,
            is_default,
        })
        .collect())
}

pub async fn add_category(pool: &PgPool, name: &str, color: Option<&str>) -> anyhow::Result<Category> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) =
        sqlx::query_as("SELECT MAX(position) FROM journal_categories")
            .fetch_one(pool)
            .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;
    sqlx::query(
        "INSERT INTO journal_categories (id, name, color, position, is_default) \
         VALUES ($1, $2, $3, $4, FALSE)",
    )
    .bind(id)
    .bind(name)
    .bind(color)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting journal category")?;
    Ok(Category {
        id,
        name: name.to_string(),
        color: color.map(str::to_string),
        description: None,
        position,
        is_default: false,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct CategoryPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub position: Option<f64>,
}

pub async fn update_category(pool: &PgPool, id: Uuid, patch: &CategoryPatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE journal_categories SET \
            name = COALESCE($2, name), \
            color = COALESCE($3, color), \
            description = COALESCE($4, description), \
            position = COALESCE($5, position) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name.as_deref())
    .bind(patch.color.as_deref())
    .bind(patch.description.as_deref())
    .bind(patch.position)
    .execute(pool)
    .await
    .context("updating journal category")?;
    Ok(res.rows_affected() > 0)
}

/// Delete a category. The default category is protected (caller checks `is_default`).
pub async fn delete_category(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_categories WHERE id = $1 AND NOT is_default")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Capital events ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CapitalEvent {
    pub id: Uuid,
    pub category_id: Uuid,
    pub kind: String,
    pub amount: f64,
    pub currency: String,
    pub note: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
}

pub const CAPITAL_KINDS: [&str; 3] = ["initial", "refill", "withdrawal"];

pub async fn list_capital_events(
    pool: &PgPool,
    category_id: Uuid,
) -> anyhow::Result<Vec<CapitalEvent>> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, f64, String, Option<String>, OffsetDateTime)>(
        "SELECT id, category_id, kind, amount, currency, note, occurred_at \
         FROM journal_capital_events WHERE category_id = $1 ORDER BY occurred_at, created_at",
    )
    .bind(category_id)
    .fetch_all(pool)
    .await
    .context("listing capital events")?;
    Ok(rows
        .into_iter()
        .map(|(id, category_id, kind, amount, currency, note, occurred_at)| CapitalEvent {
            id,
            category_id,
            kind,
            amount,
            currency,
            note,
            occurred_at,
        })
        .collect())
}

pub async fn add_capital_event(
    pool: &PgPool,
    category_id: Uuid,
    kind: &str,
    amount: f64,
    currency: &str,
    note: Option<&str>,
    occurred_at: Option<OffsetDateTime>,
) -> anyhow::Result<CapitalEvent> {
    let id = Uuid::new_v4();
    let occurred_at = occurred_at.unwrap_or_else(OffsetDateTime::now_utc);
    sqlx::query(
        "INSERT INTO journal_capital_events (id, category_id, kind, amount, currency, note, occurred_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(category_id)
    .bind(kind)
    .bind(amount)
    .bind(currency)
    .bind(note)
    .bind(occurred_at)
    .execute(pool)
    .await
    .context("inserting capital event")?;
    Ok(CapitalEvent {
        id,
        category_id,
        kind: kind.to_string(),
        amount,
        currency: currency.to_string(),
        note: note.map(str::to_string),
        occurred_at,
    })
}

pub async fn delete_capital_event(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_capital_events WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Strategies ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Strategy {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub signals: JsonValue,
    pub position: f64,
}

pub async fn list_strategies(pool: &PgPool) -> anyhow::Result<Vec<Strategy>> {
    let rows = sqlx::query_as::<_, (Uuid, String, Option<String>, JsonValue, f64)>(
        "SELECT id, name, description, signals, position \
         FROM journal_strategies ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing strategies")?;
    Ok(rows
        .into_iter()
        .map(|(id, name, description, signals, position)| Strategy {
            id,
            name,
            description,
            signals,
            position,
        })
        .collect())
}

pub async fn add_strategy(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
    signals: &JsonValue,
) -> anyhow::Result<Strategy> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) = sqlx::query_as("SELECT MAX(position) FROM journal_strategies")
        .fetch_one(pool)
        .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;
    sqlx::query(
        "INSERT INTO journal_strategies (id, name, description, signals, position) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(signals)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting strategy")?;
    Ok(Strategy {
        id,
        name: name.to_string(),
        description: description.map(str::to_string),
        signals: signals.clone(),
        position,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct StrategyPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub signals: Option<JsonValue>,
    pub position: Option<f64>,
}

pub async fn update_strategy(pool: &PgPool, id: Uuid, patch: &StrategyPatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE journal_strategies SET \
            name = COALESCE($2, name), \
            description = COALESCE($3, description), \
            signals = COALESCE($4, signals), \
            position = COALESCE($5, position) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name.as_deref())
    .bind(patch.description.as_deref())
    .bind(patch.signals.as_ref())
    .bind(patch.position)
    .execute(pool)
    .await
    .context("updating strategy")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_strategy(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_strategies WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Templates ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Template {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub fields: JsonValue,
    pub is_builtin: bool,
    pub position: f64,
    /// Pre-selected fee schedule when logging a trade from this template (overridable).
    pub default_fee_schedule_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub async fn list_templates(pool: &PgPool) -> anyhow::Result<Vec<Template>> {
    let rows = sqlx::query_as::<
        _,
        (Uuid, String, Option<String>, JsonValue, bool, f64, Option<Uuid>, OffsetDateTime),
    >(
        "SELECT id, name, description, fields, is_builtin, position, default_fee_schedule_id, \
            updated_at \
         FROM journal_templates ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing templates")?;
    Ok(rows
        .into_iter()
        .map(
            |(id, name, description, fields, is_builtin, position, default_fee_schedule_id, updated_at)| {
                Template {
                    id,
                    name,
                    description,
                    fields,
                    is_builtin,
                    position,
                    default_fee_schedule_id,
                    updated_at,
                }
            },
        )
        .collect())
}

pub async fn add_template(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
    fields: &JsonValue,
    default_fee_schedule_id: Option<Uuid>,
) -> anyhow::Result<Template> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) = sqlx::query_as("SELECT MAX(position) FROM journal_templates")
        .fetch_one(pool)
        .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;
    sqlx::query(
        "INSERT INTO journal_templates (id, name, description, fields, position, default_fee_schedule_id) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(fields)
    .bind(position)
    .bind(default_fee_schedule_id)
    .execute(pool)
    .await
    .context("inserting template")?;
    Ok(Template {
        id,
        name: name.to_string(),
        description: description.map(str::to_string),
        fields: fields.clone(),
        is_builtin: false,
        position,
        default_fee_schedule_id,
        updated_at: OffsetDateTime::now_utc(),
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct TemplatePatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub fields: Option<JsonValue>,
    pub position: Option<f64>,
    /// Present (even as null) clears/sets the default; absent leaves it unchanged.
    #[serde(default, deserialize_with = "deserialize_some")]
    pub default_fee_schedule_id: Option<Option<Uuid>>,
}

pub async fn update_template(pool: &PgPool, id: Uuid, patch: &TemplatePatch) -> anyhow::Result<bool> {
    // default_fee_schedule_id uses a double-Option so the caller can distinguish "leave
    // unchanged" (None) from "set to NULL" (Some(None)).
    let (set_default, default_val) = match patch.default_fee_schedule_id {
        Some(v) => (true, v),
        None => (false, None),
    };
    let res = sqlx::query(
        "UPDATE journal_templates SET \
            name = COALESCE($2, name), \
            description = COALESCE($3, description), \
            fields = COALESCE($4, fields), \
            position = COALESCE($5, position), \
            default_fee_schedule_id = CASE WHEN $6 THEN $7 ELSE default_fee_schedule_id END, \
            updated_at = now() \
         WHERE id = $1 AND NOT is_builtin",
    )
    .bind(id)
    .bind(patch.name.as_deref())
    .bind(patch.description.as_deref())
    .bind(patch.fields.as_ref())
    .bind(patch.position)
    .bind(set_default)
    .bind(default_val)
    .execute(pool)
    .await
    .context("updating template")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_template(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_templates WHERE id = $1 AND NOT is_builtin")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Fee schedules ────────────────────────────────────────────────────────────

/// A reusable fee template applied to a trade as a shortcut. `amount_kind` is "fixed"
/// (a currency amount) or "pct" (a percentage of notional). `per` is "lot"|"unit"|
/// "contract"|"trade". Applying it auto-computes the trade's fee (see `compute_fee`).
#[derive(Debug, Clone, Serialize)]
pub struct FeeSchedule {
    pub id: Uuid,
    pub name: String,
    pub amount: f64,
    pub amount_kind: String,
    pub per: String,
    pub currency: String,
    pub position: f64,
}

pub const FEE_AMOUNT_KINDS: [&str; 2] = ["fixed", "pct"];
pub const FEE_PER: [&str; 4] = ["lot", "unit", "contract", "trade"];

pub async fn list_fee_schedules(pool: &PgPool) -> anyhow::Result<Vec<FeeSchedule>> {
    let rows = sqlx::query_as::<_, (Uuid, String, f64, String, String, String, f64)>(
        "SELECT id, name, amount, amount_kind, per, currency, position \
         FROM journal_fee_schedules ORDER BY position, created_at",
    )
    .fetch_all(pool)
    .await
    .context("listing fee schedules")?;
    Ok(rows
        .into_iter()
        .map(|(id, name, amount, amount_kind, per, currency, position)| FeeSchedule {
            id,
            name,
            amount,
            amount_kind,
            per,
            currency,
            position,
        })
        .collect())
}

/// Fetch a single fee schedule by id (used to auto-fee triggered SL/TP brackets).
pub async fn get_fee_schedule(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<FeeSchedule>> {
    let row = sqlx::query_as::<_, (Uuid, String, f64, String, String, String, f64)>(
        "SELECT id, name, amount, amount_kind, per, currency, position \
         FROM journal_fee_schedules WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .context("fetching fee schedule")?;
    Ok(row.map(
        |(id, name, amount, amount_kind, per, currency, position)| FeeSchedule {
            id,
            name,
            amount,
            amount_kind,
            per,
            currency,
            position,
        },
    ))
}

#[derive(Debug, Deserialize)]
pub struct FeeScheduleInput {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub amount: f64,
    #[serde(default = "default_fee_kind")]
    pub amount_kind: String,
    #[serde(default = "default_fee_per")]
    pub per: String,
    #[serde(default = "default_currency")]
    pub currency: String,
}
fn default_fee_kind() -> String {
    "fixed".to_string()
}
fn default_fee_per() -> String {
    "trade".to_string()
}

pub async fn add_fee_schedule(pool: &PgPool, input: &FeeScheduleInput) -> anyhow::Result<FeeSchedule> {
    let id = Uuid::new_v4();
    let next: (Option<f64>,) = sqlx::query_as("SELECT MAX(position) FROM journal_fee_schedules")
        .fetch_one(pool)
        .await?;
    let position = next.0.unwrap_or(0.0) + 1.0;
    sqlx::query(
        "INSERT INTO journal_fee_schedules (id, name, amount, amount_kind, per, currency, position) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(&input.name)
    .bind(input.amount)
    .bind(&input.amount_kind)
    .bind(&input.per)
    .bind(&input.currency)
    .bind(position)
    .execute(pool)
    .await
    .context("inserting fee schedule")?;
    Ok(FeeSchedule {
        id,
        name: input.name.clone(),
        amount: input.amount,
        amount_kind: input.amount_kind.clone(),
        per: input.per.clone(),
        currency: input.currency.clone(),
        position,
    })
}

#[derive(Debug, Deserialize, Default)]
pub struct FeeSchedulePatch {
    pub name: Option<String>,
    pub amount: Option<f64>,
    pub amount_kind: Option<String>,
    pub per: Option<String>,
    pub currency: Option<String>,
    pub position: Option<f64>,
}

pub async fn update_fee_schedule(
    pool: &PgPool,
    id: Uuid,
    patch: &FeeSchedulePatch,
) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE journal_fee_schedules SET \
            name = COALESCE($2, name), \
            amount = COALESCE($3, amount), \
            amount_kind = COALESCE($4, amount_kind), \
            per = COALESCE($5, per), \
            currency = COALESCE($6, currency), \
            position = COALESCE($7, position), \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name.as_deref())
    .bind(patch.amount)
    .bind(patch.amount_kind.as_deref())
    .bind(patch.per.as_deref())
    .bind(patch.currency.as_deref())
    .bind(patch.position)
    .execute(pool)
    .await
    .context("updating fee schedule")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_fee_schedule(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_fee_schedules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Fee charged by a schedule for a trade of `qty` units at `avg_price`.
///
/// - `per == "trade"`: the amount is charged once (flat), regardless of size.
/// - otherwise (per lot/unit/contract): fixed → amount × qty; pct → amount% of notional
///   (avg_price × qty).
///
/// This is the single definition of the schedule math; the client mirrors it for the
/// live fee preview, and the user-entered `fees` on the trade is the stored value (so a
/// manual override always wins). Returns 0 when inputs are missing.
pub fn compute_fee(s: &FeeSchedule, qty: f64, avg_price: f64) -> f64 {
    let qty = qty.abs();
    let raw = match (s.per.as_str(), s.amount_kind.as_str()) {
        ("trade", "pct") => avg_price.abs() * qty * s.amount / 100.0,
        ("trade", _) => s.amount,
        (_, "pct") => avg_price.abs() * qty * s.amount / 100.0,
        (_, _) => s.amount * qty,
    };
    // Percentage fees keep 6 decimals; currency-amount fees keep 4. Both trim the
    // floating-point noise (e.g. 2.0000000000000004 → 2.0) so stored/shown fees match.
    let dp = if s.amount_kind == "pct" { 6 } else { 4 };
    round_dp(raw, dp)
}

/// Round `v` to `dp` decimal places (half-away-from-zero), stripping float noise.
pub fn round_dp(v: f64, dp: u32) -> f64 {
    if !v.is_finite() {
        return v;
    }
    let f = 10f64.powi(dp as i32);
    (v * f).round() / f
}

// ── Journal settings ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct Settings {
    pub display_currency: String,
}

pub async fn get_settings(pool: &PgPool) -> anyhow::Result<Settings> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT display_currency FROM journal_settings WHERE id = TRUE")
            .fetch_optional(pool)
            .await
            .context("loading journal settings")?;
    Ok(Settings {
        display_currency: row.map(|r| r.0).unwrap_or_else(|| "USD".to_string()),
    })
}

pub async fn set_display_currency(pool: &PgPool, currency: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journal_settings (id, display_currency, updated_at) \
         VALUES (TRUE, $1, now()) \
         ON CONFLICT (id) DO UPDATE SET display_currency = $1, updated_at = now()",
    )
    .bind(currency)
    .execute(pool)
    .await
    .context("setting display currency")?;
    Ok(())
}

// ── Trades ───────────────────────────────────────────────────────────────────

pub const ASSET_CLASSES: [&str; 7] =
    ["stock", "option", "crypto", "etf", "future", "forex", "other"];

/// Supported currencies (12 majors). Stored as ISO-4217 codes; "couronne" maps to the
/// Nordic crowns (SEK/NOK/DKK). A future daily FX feed will convert between these.
pub const CURRENCIES: [&str; 12] = [
    "USD", "EUR", "GBP", "JPY", "CNY", "CHF", "CAD", "AUD", "HKD", "SEK", "NOK", "DKK",
];

/// What `quantity` counts on a trade; also selects which fee-schedule rates apply.
pub const UNIT_TYPES: [&str; 4] = ["lot", "unit", "contract", "share"];

/// A trade row. `net_pnl`/`gross_pnl` are computed on read (not stored) so edits to
/// price/qty/fees stay consistent. `None` when the trade is still open (no exit).
#[derive(Debug, Serialize)]
pub struct Trade {
    pub id: Uuid,
    pub category_id: Uuid,
    pub template_id: Option<Uuid>,
    pub strategy_id: Option<Uuid>,

    pub ticker: String,
    pub asset_class: String,
    pub exchange: Option<String>,
    pub side: String,
    pub currency: String,
    pub unit_type: String,
    pub fee_schedule_id: Option<Uuid>,

    #[serde(with = "time::serde::rfc3339::option")]
    pub entry_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub exit_at: Option<OffsetDateTime>,
    pub entry_price: Option<f64>,
    pub exit_price: Option<f64>,
    pub quantity: Option<f64>,
    pub fees: f64,
    pub leverage: f64,
    pub multiplier: f64,

    pub signal_name: Option<String>,
    pub feedback: Option<String>,
    pub images: JsonValue,
    pub fields: JsonValue,

    // Multi-leg position (when `advanced`). Legs carry their own price/qty/date/fees so
    // a FIFO tax report is derivable; `cost_basis_method` selects journal display.
    pub advanced: bool,
    pub cost_basis_method: String,
    pub entries: JsonValue,
    pub exits: JsonValue,
    pub brackets: JsonValue,

    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    // Computed:
    pub gross_pnl: Option<f64>,
    pub net_pnl: Option<f64>,
    /// Average entry price across entry legs (or the flat entry in simple mode).
    pub avg_entry: Option<f64>,
    /// Total quantity still open (entry qty − exit qty); > 0 means partially open.
    pub open_qty: f64,
}

/// One entry or exit fill. `at`/`fees`/`signal` optional; `qty` is positive size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Leg {
    #[serde(default)]
    pub id: String,
    pub price: f64,
    pub qty: f64,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub at: Option<OffsetDateTime>,
    #[serde(default)]
    pub fees: f64,
    #[serde(default)]
    pub signal: Option<String>,
}

/// A planned stop-loss / take-profit. No signal (not signal-driven). When `triggered`
/// the API folds it into an exit leg so PnL has a single source of truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bracket {
    #[serde(default)]
    pub id: String,
    /// "sl" | "tp"
    pub kind: String,
    pub price: f64,
    #[serde(default)]
    pub qty: Option<f64>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub at: Option<OffsetDateTime>,
    #[serde(default)]
    pub triggered: bool,
    #[serde(default)]
    pub note: Option<String>,
}

/// The stored trade columns. sqlx tuples cap at 16 elements (we have more), so we
/// derive `FromRow` on a struct and convert into the `Trade` API type, computing PnL.
#[derive(sqlx::FromRow)]
struct TradeRow {
    id: Uuid,
    category_id: Uuid,
    template_id: Option<Uuid>,
    strategy_id: Option<Uuid>,
    ticker: String,
    asset_class: String,
    exchange: Option<String>,
    side: String,
    currency: String,
    unit_type: String,
    fee_schedule_id: Option<Uuid>,
    entry_at: Option<OffsetDateTime>,
    exit_at: Option<OffsetDateTime>,
    entry_price: Option<f64>,
    exit_price: Option<f64>,
    quantity: Option<f64>,
    fees: f64,
    leverage: f64,
    multiplier: f64,
    signal_name: Option<String>,
    feedback: Option<String>,
    images: JsonValue,
    fields: JsonValue,
    advanced: bool,
    cost_basis_method: String,
    entries: JsonValue,
    exits: JsonValue,
    brackets: JsonValue,
    created_at: OffsetDateTime,
}

const TRADE_COLUMNS: &str = "id, category_id, template_id, strategy_id, ticker, asset_class, \
    exchange, side, currency, unit_type, fee_schedule_id, entry_at, exit_at, entry_price, \
    exit_price, quantity, fees, leverage, multiplier, signal_name, feedback, images, fields, \
    advanced, cost_basis_method, entries, exits, brackets, created_at";

fn row_to_trade(r: TradeRow) -> Trade {
    // Normalize to entry/exit legs: advanced trades use the JSON arrays; simple trades
    // synthesize one leg each from the flat fields, so PnL has a single code path.
    let (entries, exits) = if r.advanced {
        (
            serde_json::from_value::<Vec<Leg>>(r.entries.clone()).unwrap_or_default(),
            serde_json::from_value::<Vec<Leg>>(r.exits.clone()).unwrap_or_default(),
        )
    } else {
        let entry = match (r.entry_price, r.quantity) {
            (Some(price), Some(qty)) => vec![Leg {
                id: String::new(),
                price,
                qty,
                at: r.entry_at,
                fees: 0.0,
                signal: None,
            }],
            _ => vec![],
        };
        let exit = match (r.exit_price, r.quantity) {
            (Some(price), Some(qty)) => vec![Leg {
                id: String::new(),
                price,
                qty,
                at: r.exit_at,
                fees: 0.0,
                signal: None,
            }],
            _ => vec![],
        };
        (entry, exit)
    };

    let pnl = compute_pnl(&r.side, &entries, &exits, r.multiplier, r.fees);

    Trade {
        id: r.id,
        category_id: r.category_id,
        template_id: r.template_id,
        strategy_id: r.strategy_id,
        ticker: r.ticker,
        asset_class: r.asset_class,
        exchange: r.exchange,
        side: r.side,
        currency: r.currency,
        unit_type: r.unit_type,
        fee_schedule_id: r.fee_schedule_id,
        entry_at: r.entry_at,
        exit_at: r.exit_at,
        entry_price: r.entry_price,
        exit_price: r.exit_price,
        quantity: r.quantity,
        fees: r.fees,
        leverage: r.leverage,
        multiplier: r.multiplier,
        signal_name: r.signal_name,
        feedback: r.feedback,
        images: r.images,
        fields: r.fields,
        advanced: r.advanced,
        cost_basis_method: r.cost_basis_method,
        entries: r.entries,
        exits: r.exits,
        brackets: r.brackets,
        created_at: r.created_at,
        gross_pnl: pnl.gross,
        net_pnl: pnl.net,
        avg_entry: pnl.avg_entry,
        open_qty: pnl.open_qty,
    }
}

struct PnlResult {
    gross: Option<f64>,
    net: Option<f64>,
    avg_entry: Option<f64>,
    open_qty: f64,
}

/// Weighted-average-cost realized PnL over entry/exit legs.
///
/// avg_entry = Σ(price·qty)/Σ(qty); each exit realizes (exit−avg_entry)·qty·mult·dir.
/// Net subtracts every leg's fees plus the trade-level `fees`. PnL is realized on the
/// closed portion immediately (partial scale-outs count now); `open_qty` flags any
/// remaining open size. Returns `None` PnL when there are no entries or no exits yet.
fn compute_pnl(side: &str, entries: &[Leg], exits: &[Leg], multiplier: f64, trade_fees: f64) -> PnlResult {
    let entry_qty: f64 = entries.iter().map(|l| l.qty).sum();
    let exit_qty: f64 = exits.iter().map(|l| l.qty).sum();
    let open_qty = entry_qty - exit_qty;

    if entry_qty <= 0.0 {
        return PnlResult { gross: None, net: None, avg_entry: None, open_qty: 0.0 };
    }
    let avg_entry = entries.iter().map(|l| l.price * l.qty).sum::<f64>() / entry_qty;

    if exits.is_empty() {
        // Position open, nothing realized yet.
        return PnlResult { gross: None, net: None, avg_entry: Some(avg_entry), open_qty };
    }

    let dir = if side == "short" { -1.0 } else { 1.0 };
    let gross: f64 = exits
        .iter()
        .map(|l| (l.price - avg_entry) * l.qty * multiplier * dir)
        .sum();
    let leg_fees: f64 = entries.iter().map(|l| l.fees).sum::<f64>()
        + exits.iter().map(|l| l.fees).sum::<f64>();
    let net = gross - leg_fees - trade_fees;

    PnlResult {
        gross: Some(gross),
        net: Some(net),
        avg_entry: Some(avg_entry),
        open_qty: open_qty.max(0.0),
    }
}

/// Full editable trade payload from the client.
#[derive(Debug, Deserialize, Default)]
pub struct TradeInput {
    pub category_id: Uuid,
    pub template_id: Option<Uuid>,
    pub strategy_id: Option<Uuid>,
    #[serde(default)]
    pub ticker: String,
    #[serde(default = "default_asset")]
    pub asset_class: String,
    pub exchange: Option<String>,
    #[serde(default = "default_side")]
    pub side: String,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_unit")]
    pub unit_type: String,
    pub fee_schedule_id: Option<Uuid>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub entry_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub exit_at: Option<OffsetDateTime>,
    pub entry_price: Option<f64>,
    pub exit_price: Option<f64>,
    pub quantity: Option<f64>,
    #[serde(default)]
    pub fees: f64,
    #[serde(default = "default_one")]
    pub leverage: f64,
    #[serde(default = "default_one")]
    pub multiplier: f64,
    pub signal_name: Option<String>,
    pub feedback: Option<String>,
    #[serde(default = "empty_array")]
    pub images: JsonValue,
    #[serde(default = "empty_object")]
    pub fields: JsonValue,

    // Multi-leg (advanced) fields.
    #[serde(default)]
    pub advanced: bool,
    #[serde(default = "default_cost_basis")]
    pub cost_basis_method: String,
    #[serde(default)]
    pub entries: Vec<Leg>,
    #[serde(default)]
    pub exits: Vec<Leg>,
    #[serde(default)]
    pub brackets: Vec<Bracket>,
}

fn default_cost_basis() -> String {
    "avg".to_string()
}
fn default_currency() -> String {
    "USD".to_string()
}
fn default_unit() -> String {
    "unit".to_string()
}

impl TradeInput {
    /// Fold triggered SL/TP brackets into exit legs so PnL has a single source of
    /// truth. A triggered bracket without an explicit qty closes the remaining open
    /// quantity. Only runs for advanced trades; simple trades are untouched. Idempotent
    /// in spirit: a bracket already represented as an exit just adds its qty.
    ///
    /// When a `schedule` is given, each folded exit's fee is auto-computed from it
    /// (qty × bracket price), so the SL/TP fill carries its own fee into the realized
    /// PnL instead of silently costing nothing. Mirrors the client preview.
    pub fn normalize(&mut self, schedule: Option<&FeeSchedule>) {
        if !self.advanced {
            return;
        }
        let entry_qty: f64 = self.entries.iter().map(|l| l.qty).sum();
        let mut exit_qty: f64 = self.exits.iter().map(|l| l.qty).sum();
        for b in &self.brackets {
            if !b.triggered {
                continue;
            }
            // Skip if an exit at this bracket's id was already materialized.
            let synthetic_id = format!("bracket:{}", b.id);
            if self.exits.iter().any(|e| e.id == synthetic_id) {
                continue;
            }
            let remaining = (entry_qty - exit_qty).max(0.0);
            let qty = b.qty.unwrap_or(remaining).min(remaining);
            if qty <= 0.0 {
                continue;
            }
            exit_qty += qty;
            let fees = schedule.map(|s| compute_fee(s, qty, b.price)).unwrap_or(0.0);
            self.exits.push(Leg {
                id: synthetic_id,
                price: b.price,
                qty,
                at: b.at,
                fees,
                signal: None,
            });
        }
    }
}

fn default_asset() -> String {
    "stock".to_string()
}
fn default_side() -> String {
    "long".to_string()
}
fn default_one() -> f64 {
    1.0
}
fn empty_array() -> JsonValue {
    JsonValue::Array(vec![])
}
fn empty_object() -> JsonValue {
    JsonValue::Object(Default::default())
}

#[derive(Debug, Default, Deserialize)]
pub struct TradeFilter {
    pub category_id: Option<Uuid>,
    pub strategy_id: Option<Uuid>,
    pub asset_class: Option<String>,
    pub side: Option<String>,
    /// Case-insensitive exact ticker match (trimmed).
    pub ticker: Option<String>,
    /// Case-insensitive exact signal-name match (trimmed).
    pub signal_name: Option<String>,
    /// Date range on the trade's effective date (exit_at, else entry_at, else created_at).
    pub since: Option<OffsetDateTime>,
    pub until: Option<OffsetDateTime>,
}

pub async fn list_trades(pool: &PgPool, filter: &TradeFilter) -> anyhow::Result<Vec<Trade>> {
    // Effective date for ranking/filtering: exit, else entry, else creation.
    let sql = format!(
        "SELECT {TRADE_COLUMNS} FROM journal_trades \
         WHERE ($1::uuid IS NULL OR category_id = $1) \
           AND ($2::uuid IS NULL OR strategy_id = $2) \
           AND ($3::text IS NULL OR asset_class = $3) \
           AND ($4::text IS NULL OR side = $4) \
           AND ($5::text IS NULL OR lower(ticker) = lower($5)) \
           AND ($6::text IS NULL OR lower(signal_name) = lower($6)) \
           AND ($7::timestamptz IS NULL OR COALESCE(exit_at, entry_at, created_at) >= $7) \
           AND ($8::timestamptz IS NULL OR COALESCE(exit_at, entry_at, created_at) <= $8) \
         ORDER BY COALESCE(exit_at, entry_at, created_at) DESC, created_at DESC"
    );
    let rows = sqlx::query_as::<_, TradeRow>(AssertSqlSafe(sql))
        .bind(filter.category_id)
        .bind(filter.strategy_id)
        .bind(filter.asset_class.as_deref())
        .bind(filter.side.as_deref())
        .bind(filter.ticker.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .bind(
            filter
                .signal_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        )
        .bind(filter.since)
        .bind(filter.until)
        .fetch_all(pool)
        .await
        .context("listing trades")?;
    Ok(rows.into_iter().map(row_to_trade).collect())
}

pub async fn get_trade(pool: &PgPool, id: Uuid) -> anyhow::Result<Option<Trade>> {
    let sql = format!("SELECT {TRADE_COLUMNS} FROM journal_trades WHERE id = $1");
    let row = sqlx::query_as::<_, TradeRow>(AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("fetching trade")?;
    Ok(row.map(row_to_trade))
}

/// Serialize the typed leg vectors to JSONB for storage (infallible for these types).
fn legs_json<T: Serialize>(legs: &[T]) -> JsonValue {
    serde_json::to_value(legs).unwrap_or_else(|_| JsonValue::Array(vec![]))
}

pub async fn add_trade(pool: &PgPool, input: &TradeInput) -> anyhow::Result<Trade> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO journal_trades \
            (id, category_id, template_id, strategy_id, ticker, asset_class, exchange, side, \
             currency, unit_type, fee_schedule_id, entry_at, exit_at, entry_price, exit_price, \
             quantity, fees, leverage, multiplier, signal_name, feedback, images, fields, \
             advanced, cost_basis_method, entries, exits, brackets) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,\
             $21,$22,$23,$24,$25,$26,$27,$28)",
    )
    .bind(id)
    .bind(input.category_id)
    .bind(input.template_id)
    .bind(input.strategy_id)
    .bind(&input.ticker)
    .bind(&input.asset_class)
    .bind(input.exchange.as_deref())
    .bind(&input.side)
    .bind(&input.currency)
    .bind(&input.unit_type)
    .bind(input.fee_schedule_id)
    .bind(input.entry_at)
    .bind(input.exit_at)
    .bind(input.entry_price)
    .bind(input.exit_price)
    .bind(input.quantity)
    .bind(input.fees)
    .bind(input.leverage)
    .bind(input.multiplier)
    .bind(input.signal_name.as_deref())
    .bind(input.feedback.as_deref())
    .bind(&input.images)
    .bind(&input.fields)
    .bind(input.advanced)
    .bind(&input.cost_basis_method)
    .bind(legs_json(&input.entries))
    .bind(legs_json(&input.exits))
    .bind(legs_json(&input.brackets))
    .execute(pool)
    .await
    .context("inserting trade")?;
    get_trade(pool, id)
        .await?
        .context("trade vanished after insert")
}

pub async fn update_trade(pool: &PgPool, id: Uuid, input: &TradeInput) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE journal_trades SET \
            category_id = $2, template_id = $3, strategy_id = $4, ticker = $5, asset_class = $6, \
            exchange = $7, side = $8, currency = $9, unit_type = $10, fee_schedule_id = $11, \
            entry_at = $12, exit_at = $13, entry_price = $14, exit_price = $15, quantity = $16, \
            fees = $17, leverage = $18, multiplier = $19, signal_name = $20, feedback = $21, \
            images = $22, fields = $23, advanced = $24, cost_basis_method = $25, entries = $26, \
            exits = $27, brackets = $28, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(input.category_id)
    .bind(input.template_id)
    .bind(input.strategy_id)
    .bind(&input.ticker)
    .bind(&input.asset_class)
    .bind(input.exchange.as_deref())
    .bind(&input.side)
    .bind(&input.currency)
    .bind(&input.unit_type)
    .bind(input.fee_schedule_id)
    .bind(input.entry_at)
    .bind(input.exit_at)
    .bind(input.entry_price)
    .bind(input.exit_price)
    .bind(input.quantity)
    .bind(input.fees)
    .bind(input.leverage)
    .bind(input.multiplier)
    .bind(input.signal_name.as_deref())
    .bind(input.feedback.as_deref())
    .bind(&input.images)
    .bind(&input.fields)
    .bind(input.advanced)
    .bind(&input.cost_basis_method)
    .bind(legs_json(&input.entries))
    .bind(legs_json(&input.exits))
    .bind(legs_json(&input.brackets))
    .execute(pool)
    .await
    .context("updating trade")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_trade(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_trades WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

// ── Autocomplete suggestions ─────────────────────────────────────────────────

/// Distinct, previously-used free-text values to offer as autocomplete when logging a
/// new trade (tickers, exchanges, signal names). Most recent first.
#[derive(Debug, Serialize)]
pub struct TradeSuggestions {
    pub tickers: Vec<String>,
    pub exchanges: Vec<String>,
    pub signals: Vec<String>,
}

/// Pull distinct non-empty values for one column, newest-used first.
async fn distinct_values(pool: &PgPool, column: &str) -> anyhow::Result<Vec<String>> {
    // `column` is a fixed internal identifier (never user input), so interpolation is safe.
    let sql = format!(
        "SELECT val FROM ( \
            SELECT {column} AS val, MAX(created_at) AS last_used \
            FROM journal_trades \
            WHERE {column} IS NOT NULL AND {column} <> '' \
            GROUP BY {column} \
         ) s ORDER BY last_used DESC LIMIT 500"
    );
    let rows: Vec<(String,)> = sqlx::query_as(AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .with_context(|| format!("listing distinct {column}"))?;
    Ok(rows.into_iter().map(|(v,)| v).collect())
}

pub async fn trade_suggestions(pool: &PgPool) -> anyhow::Result<TradeSuggestions> {
    Ok(TradeSuggestions {
        tickers: distinct_values(pool, "ticker").await?,
        exchanges: distinct_values(pool, "exchange").await?,
        signals: distinct_values(pool, "signal_name").await?,
    })
}

// ── Performance breakdown ────────────────────────────────────────────────────

/// A point on the cumulative equity curve (realized PnL added to invested capital).
#[derive(Debug, Serialize)]
pub struct EquityPoint {
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
    /// Equity after this event = invested capital so far + cumulative realized PnL.
    pub equity: f64,
    /// Cumulative realized net PnL up to this point.
    pub cum_pnl: f64,
}

/// Aggregate stats for a category (or all categories when `category_id` is None).
#[derive(Debug, Serialize)]
pub struct Breakdown {
    pub invested_capital: f64,
    pub realized_pnl: f64,
    pub total_fees: f64,
    pub current_equity: f64,
    pub return_pct: Option<f64>,
    pub trade_count: i64,
    pub closed_count: i64,
    pub open_count: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub win_rate: Option<f64>,
    pub avg_win: Option<f64>,
    pub avg_loss: Option<f64>,
    pub profit_factor: Option<f64>,
    pub expectancy: Option<f64>,
    /// Sharpe over per-trade returns (relative to invested capital), unannualized.
    pub sharpe: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub best_trade: Option<f64>,
    pub worst_trade: Option<f64>,
    /// Total margin deployed across closed trades: sum of notional / leverage.
    pub total_margin: f64,
    /// Realized PnL as a percentage of total margin deployed (leverage-aware return).
    pub return_on_margin: Option<f64>,
    pub equity_curve: Vec<EquityPoint>,

    /// Currency all monetary figures above are expressed in (after FX conversion).
    pub display_currency: String,
    /// Closed trades excluded from the totals because their FX rate was unavailable for
    /// the trade's date. Their dates are queued as pending tasks for the user to fill in.
    pub unconverted_trades: i64,
}

/// Margin tied up by a closed trade: notional (|entry| * qty * multiplier) divided by
/// leverage. Returns 0 when the trade lacks the inputs needed to size it.
fn trade_margin(t: &Trade) -> f64 {
    // Notional = avg entry price × total entry quantity × multiplier; margin divides by
    // leverage. Works for both simple and advanced trades via the computed avg_entry.
    let entry_qty = if t.advanced {
        serde_json::from_value::<Vec<Leg>>(t.entries.clone())
            .unwrap_or_default()
            .iter()
            .map(|l| l.qty)
            .sum::<f64>()
    } else {
        t.quantity.unwrap_or(0.0)
    };
    match t.avg_entry {
        Some(avg) if entry_qty > 0.0 => {
            let lev = if t.leverage > 0.0 { t.leverage } else { 1.0 };
            (avg.abs() * entry_qty.abs() * t.multiplier) / lev
        }
        _ => 0.0,
    }
}

/// The date used to price a trade's FX conversion: its exit, else entry, else creation.
fn trade_effective_date(t: &Trade) -> time::Date {
    t.exit_at.or(t.entry_at).unwrap_or(t.created_at).date()
}

/// Compute the full performance breakdown, expressed in `display_currency`. Combines
/// invested capital (sum of capital events) with realized PnL from closed trades, ordered
/// by exit time, to build an equity curve and the derived risk/return metrics.
///
/// Monetary values are FX-converted into `display_currency` using each trade's / capital
/// event's effective-date rate (carry-forward). A closed trade whose rate is unavailable is
/// excluded from the totals and its date queued as a pending task.
pub async fn breakdown(
    pool: &PgPool,
    filter: &TradeFilter,
    display_currency: &str,
) -> anyhow::Result<Breakdown> {
    // Invested capital is a property of the category (its deposits), independent of the
    // trade-level filters. Convert each event into the display currency by its date.
    let cap_events = sqlx::query_as::<_, (f64, String, OffsetDateTime, String)>(
        "SELECT amount, currency, occurred_at, kind FROM journal_capital_events \
         WHERE ($1::uuid IS NULL OR category_id = $1)",
    )
    .bind(filter.category_id)
    .fetch_all(pool)
    .await
    .context("loading capital events")?;
    let mut invested_capital = 0.0;
    for (amount, currency, occurred_at, kind) in &cap_events {
        let signed = if kind == "withdrawal" { -amount } else { *amount };
        if let Some(v) =
            crate::journal_fx::convert(pool, signed, currency, display_currency, occurred_at.date())
                .await?
        {
            invested_capital += v;
        }
        // Unconvertible capital is simply omitted; the FX job queues its date as pending.
    }

    let mut trades = list_trades(pool, filter).await?;

    // Order closed trades chronologically by exit for the equity curve.
    trades.sort_by_key(|t| t.exit_at.or(t.entry_at).unwrap_or(t.created_at));

    let trade_count = trades.len() as i64;

    // Convert each trade's PnL/fees/margin into the display currency. Closed trades whose
    // rate is missing are excluded and their date flagged as a pending task.
    struct Conv<'a> {
        t: &'a Trade,
        net: f64,
    }
    let mut closed: Vec<Conv> = Vec::new();
    let mut total_fees = 0.0;
    let mut total_margin = 0.0;
    let mut unconverted_trades = 0i64;
    for t in &trades {
        let date = trade_effective_date(t);
        // Fees apply to every trade (open or closed) that has any.
        if let Some(f) =
            crate::journal_fx::convert(pool, t.fees, &t.currency, display_currency, date).await?
        {
            total_fees += f;
        }
        let Some(native_net) = t.net_pnl else {
            continue; // still open
        };
        let net = crate::journal_fx::convert(pool, native_net, &t.currency, display_currency, date)
            .await?;
        match net {
            Some(net) => {
                if let Some(m) = crate::journal_fx::convert(
                    pool,
                    trade_margin(t),
                    &t.currency,
                    display_currency,
                    date,
                )
                .await?
                {
                    total_margin += m;
                }
                closed.push(Conv { t, net });
            }
            None => {
                // No rate for this date — exclude from totals, queue it for the user.
                crate::journal_fx::mark_pending(pool, date, "needed to convert a trade").await?;
                unconverted_trades += 1;
            }
        }
    }

    let closed_count = closed.len() as i64;
    let open_count = trade_count - closed_count - unconverted_trades;

    let pnls: Vec<f64> = closed.iter().map(|c| c.net).collect();
    let realized_pnl: f64 = pnls.iter().sum();

    let wins: Vec<f64> = pnls.iter().copied().filter(|p| *p > 0.0).collect();
    let losses: Vec<f64> = pnls.iter().copied().filter(|p| *p < 0.0).collect();
    let win_count = wins.len() as i64;
    let loss_count = losses.len() as i64;

    let win_rate = (closed_count > 0).then(|| win_count as f64 / closed_count as f64);
    let avg_win = (!wins.is_empty()).then(|| wins.iter().sum::<f64>() / wins.len() as f64);
    let avg_loss = (!losses.is_empty()).then(|| losses.iter().sum::<f64>() / losses.len() as f64);

    let gross_win: f64 = wins.iter().sum();
    let gross_loss: f64 = losses.iter().map(|l| l.abs()).sum();
    let profit_factor = (gross_loss > 0.0).then(|| gross_win / gross_loss);

    let expectancy = (closed_count > 0).then(|| realized_pnl / closed_count as f64);

    // Sharpe over per-trade returns relative to invested capital (unannualized):
    // mean(return) / stddev(return). Needs >= 2 trades and capital to normalize.
    let sharpe = if pnls.len() >= 2 && invested_capital > 0.0 {
        let rets: Vec<f64> = pnls.iter().map(|p| p / invested_capital).collect();
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len() as f64 - 1.0);
        let sd = var.sqrt();
        (sd > 0.0).then(|| mean / sd)
    } else {
        None
    };

    // Equity curve + max drawdown over cumulative realized PnL (in display currency).
    let mut equity_curve = Vec::with_capacity(closed.len());
    let mut cum = 0.0;
    let mut peak = invested_capital;
    let mut max_dd = 0.0_f64;
    for c in &closed {
        cum += c.net;
        let equity = invested_capital + cum;
        peak = peak.max(equity);
        if peak > 0.0 {
            max_dd = max_dd.max((peak - equity) / peak);
        }
        equity_curve.push(EquityPoint {
            at: c.t.exit_at.or(c.t.entry_at).unwrap_or(c.t.created_at),
            equity,
            cum_pnl: cum,
        });
    }

    let current_equity = invested_capital + realized_pnl;
    let return_pct = (invested_capital > 0.0).then(|| realized_pnl / invested_capital * 100.0);
    let max_drawdown = (!closed.is_empty()).then_some(max_dd * 100.0);
    let best_trade = pnls.iter().copied().reduce(f64::max);
    let worst_trade = pnls.iter().copied().reduce(f64::min);

    // Leverage-aware return: PnL against the margin actually deployed (already converted).
    let return_on_margin = (total_margin > 0.0).then(|| realized_pnl / total_margin * 100.0);

    Ok(Breakdown {
        invested_capital,
        realized_pnl,
        total_fees,
        current_equity,
        return_pct,
        trade_count,
        closed_count,
        open_count,
        win_count,
        loss_count,
        win_rate,
        avg_win,
        avg_loss,
        profit_factor,
        expectancy,
        sharpe,
        max_drawdown,
        best_trade,
        worst_trade,
        total_margin,
        return_on_margin,
        equity_curve,
        display_currency: display_currency.to_string(),
        unconverted_trades,
    })
}
