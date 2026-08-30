//! Analytics and discipline tagging for the Trading Journal.
//!
//! Everything here is derived from trades the user already logged: no candles are read,
//! no price is inferred. Two inputs make the whole file work:
//!
//! * the **planned stop** (`journal_trades.stop_price`, or the first SL bracket on an
//!   advanced trade) — risk per trade, and therefore R-multiples;
//! * **tags** (`journal_tags`, kinds `mistake` / `rule` / `setup`) — the discipline loop.
//!   A mistake tag turns "I over-traded again" into a number: what those trades made
//!   against what the untagged ones make on average.
//!
//! Money is FX-converted into the journal's display currency exactly like `breakdown`
//! does (per-trade effective date, carry-forward); ratios that are currency-free (R,
//! win rate) are computed natively and never touch the FX table.
//!
//! Times are bucketed in the *viewer's* local zone (`tz_offset_min`, the JS
//! `getTimezoneOffset()` convention: minutes to add to local to get UTC), so "Tuesday"
//! and "the 9 a.m. hour" mean what the user sees, matching the calendar heatmap.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::{BTreeMap, HashMap};
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

use crate::journal::{self, Bracket, Trade, TradeFilter};

// ── Tags ─────────────────────────────────────────────────────────────────────

/// Tag kinds. `mistake` = a rule broken, `rule` = a rule honoured, `setup` = neutral
/// classification. The kind only changes how the UI reads the stat, never the math.
pub const TAG_KINDS: [&str; 3] = ["mistake", "rule", "setup"];

#[derive(Debug, Serialize)]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub color: Option<String>,
    pub description: Option<String>,
    pub position: f64,
    /// How many trades currently carry this tag.
    pub trade_count: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TagInput {
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub color: Option<String>,
    pub description: Option<String>,
}

fn default_kind() -> String {
    "mistake".to_string()
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct TagPatch {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub position: Option<f64>,
}

pub async fn list_tags(pool: &PgPool) -> anyhow::Result<Vec<Tag>> {
    let rows = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>, f64, i64)>(
        "SELECT t.id, t.name, t.kind, t.color, t.description, t.position, \
                COUNT(tt.trade_id) AS trade_count \
         FROM journal_tags t \
         LEFT JOIN journal_trade_tags tt ON tt.tag_id = t.id \
         GROUP BY t.id \
         ORDER BY t.kind, t.position, t.name",
    )
    .fetch_all(pool)
    .await
    .context("listing journal tags")?;
    Ok(rows
        .into_iter()
        .map(|(id, name, kind, color, description, position, trade_count)| Tag {
            id,
            name,
            kind,
            color,
            description,
            position,
            trade_count,
        })
        .collect())
}

pub async fn add_tag(pool: &PgPool, input: &TagInput) -> anyhow::Result<Tag> {
    let id = Uuid::new_v4();
    // Next position computed inside the INSERT so two concurrent adds can't collide.
    let (position,): (f64,) = sqlx::query_as(
        "INSERT INTO journal_tags (id, name, kind, color, description, position) \
         VALUES ($1, $2, $3, $4, $5, \
             (SELECT COALESCE(MAX(position), 0) + 1 FROM journal_tags)) \
         RETURNING position",
    )
    .bind(id)
    .bind(input.name.trim())
    .bind(&input.kind)
    .bind(input.color.as_deref())
    .bind(input.description.as_deref())
    .fetch_one(pool)
    .await
    .context("inserting journal tag")?;
    Ok(Tag {
        id,
        name: input.name.trim().to_string(),
        kind: input.kind.clone(),
        color: input.color.clone(),
        description: input.description.clone(),
        position,
        trade_count: 0,
    })
}

pub async fn update_tag(pool: &PgPool, id: Uuid, patch: &TagPatch) -> anyhow::Result<bool> {
    let res = sqlx::query(
        "UPDATE journal_tags SET \
            name = COALESCE($2, name), \
            kind = COALESCE($3, kind), \
            color = COALESCE($4, color), \
            description = COALESCE($5, description), \
            position = COALESCE($6, position) \
         WHERE id = $1",
    )
    .bind(id)
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.kind.as_deref())
    .bind(patch.color.as_deref())
    .bind(patch.description.as_deref())
    .bind(patch.position)
    .execute(pool)
    .await
    .context("updating journal tag")?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_tag(pool: &PgPool, id: Uuid) -> anyhow::Result<bool> {
    let res = sqlx::query("DELETE FROM journal_tags WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .context("deleting journal tag")?;
    Ok(res.rows_affected() > 0)
}

/// Replace a trade's tag set. Unknown ids are dropped by the FK, so a stale client
/// payload cannot fail the save.
pub async fn set_trade_tags(pool: &PgPool, trade_id: Uuid, tags: &[Uuid]) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM journal_trade_tags WHERE trade_id = $1")
        .bind(trade_id)
        .execute(pool)
        .await
        .context("clearing trade tags")?;
    if tags.is_empty() {
        return Ok(());
    }
    let mut unique: Vec<Uuid> = tags.to_vec();
    unique.sort();
    unique.dedup();
    sqlx::query(
        "INSERT INTO journal_trade_tags (trade_id, tag_id) \
         SELECT $1, t.id FROM journal_tags t WHERE t.id = ANY($2) \
         ON CONFLICT DO NOTHING",
    )
    .bind(trade_id)
    .bind(&unique)
    .execute(pool)
    .await
    .context("attaching trade tags")?;
    Ok(())
}

/// Fill `Trade::tags` for a batch of trades in one query (never per row).
pub async fn hydrate_tags(pool: &PgPool, trades: &mut [Trade]) -> anyhow::Result<()> {
    if trades.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = trades.iter().map(|t| t.id).collect();
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT tt.trade_id, tt.tag_id FROM journal_trade_tags tt \
         JOIN journal_tags t ON t.id = tt.tag_id \
         WHERE tt.trade_id = ANY($1) \
         ORDER BY t.kind, t.position, t.name",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .context("loading trade tags")?;
    let mut by_trade: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (trade_id, tag_id) in rows {
        by_trade.entry(trade_id).or_default().push(tag_id);
    }
    for t in trades.iter_mut() {
        if let Some(v) = by_trade.remove(&t.id) {
            t.tags = v;
        }
    }
    Ok(())
}

/// id → (name, kind) for every tag, to label the per-tag stat rows.
async fn tag_meta(pool: &PgPool) -> anyhow::Result<HashMap<Uuid, (String, String)>> {
    let rows: Vec<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, name, kind FROM journal_tags")
            .fetch_all(pool)
            .await
            .context("loading tag names")?;
    Ok(rows.into_iter().map(|(id, n, k)| (id, (n, k))).collect())
}

// ── Risk per trade (the input R-multiples need) ───────────────────────────────

/// The planned stop of a trade: the explicit `stop_price` (simple trades), else the
/// first stop-loss bracket (advanced trades). `None` when the user planned no stop.
pub fn trade_stop(t: &Trade) -> Option<f64> {
    if let Some(sp) = t.stop_price {
        if sp > 0.0 {
            return Some(sp);
        }
    }
    serde_json::from_value::<Vec<Bracket>>(t.brackets.clone())
        .ok()?
        .into_iter()
        .find(|b| b.kind == "sl" && b.price > 0.0)
        .map(|b| b.price)
}

/// Currency risked if the stop had filled: |avg entry − stop| × entry qty × multiplier,
/// in the trade's own currency. `None` when there is no stop, no entry, or no size.
pub fn trade_risk(t: &Trade) -> Option<f64> {
    let stop = trade_stop(t)?;
    let entry = t.avg_entry?;
    let qty = journal::trade_entry_qty(t);
    let risk = (entry - stop).abs() * qty.abs() * t.multiplier;
    (risk > 0.0).then_some(risk)
}

/// Notional exposure at entry, in the trade's own currency.
fn trade_notional(t: &Trade) -> f64 {
    match t.avg_entry {
        Some(avg) => avg.abs() * journal::trade_entry_qty(t).abs() * t.multiplier,
        None => 0.0,
    }
}

// ── Shared closed-trade projection ────────────────────────────────────────────

/// One closed trade reduced to the scalars every metric below needs. Owns its keys so
/// the loader can drop the `Trade` vector.
struct Closed {
    id: Uuid,
    net: f64,
    fees: f64,
    /// R multiple = net PnL / risk, both in the trade's own currency (FX-free ratio).
    r: Option<f64>,
    /// Risk as a share of invested capital, when both are known.
    risk_pct: Option<f64>,
    /// Local effective timestamp (exit, else entry, else creation).
    at: OffsetDateTime,
    /// The same instant, unshifted: what a client turns back into its own local time.
    at_utc: OffsetDateTime,
    /// Local entry timestamp, when the trade recorded one.
    entry_at: Option<OffsetDateTime>,
    /// Entry timestamp, unshifted.
    entry_at_utc: Option<OffsetDateTime>,
    hold_min: Option<i64>,
    /// Entry notional, converted into the display currency.
    notional: f64,
    /// Entry quantity, average entry and exit price, in the trade's own units.
    qty: f64,
    leverage: f64,
    entry_price: Option<f64>,
    exit_price: Option<f64>,
    strategy_id: Option<Uuid>,
    ticker: String,
    asset_class: String,
    side: String,
    tags: Vec<Uuid>,
}

struct Loaded {
    closed: Vec<Closed>,
    invested_capital: f64,
    trade_count: i64,
    open_count: i64,
    unconverted: i64,
}

/// Invested capital for the filtered scope (sum of the category's capital events,
/// converted). Mirrors `breakdown`: capital is a property of the category, not of the
/// trade filters.
async fn invested_capital(
    pool: &PgPool,
    fx: &mut crate::journal_fx::FxCache,
    category_id: Option<Uuid>,
    display_currency: &str,
) -> anyhow::Result<f64> {
    let events = sqlx::query_as::<_, (f64, String, OffsetDateTime, String)>(
        "SELECT amount, currency, occurred_at, kind FROM journal_capital_events \
         WHERE ($1::uuid IS NULL OR category_id = $1)",
    )
    .bind(category_id)
    .fetch_all(pool)
    .await
    .context("loading capital events")?;
    let mut total = 0.0;
    for (amount, currency, occurred_at, kind) in &events {
        let signed = if kind == "withdrawal" { -amount } else { *amount };
        if let Some(v) = fx
            .convert(pool, signed, currency, display_currency, occurred_at.date())
            .await?
        {
            total += v;
        }
    }
    Ok(total)
}

/// Load the filtered trades and project every closed one into `Closed`, chronologically.
async fn load(
    pool: &PgPool,
    filter: &TradeFilter,
    display_currency: &str,
    tz_offset_min: i32,
) -> anyhow::Result<Loaded> {
    let trades = journal::list_trades(pool, filter).await?;
    let offset = Duration::minutes(tz_offset_min as i64);
    let mut fx = crate::journal_fx::FxCache::new();
    let capital = invested_capital(pool, &mut fx, filter.category_id, display_currency).await?;

    let mut closed = Vec::new();
    let mut open_count = 0i64;
    let mut unconverted = 0i64;
    for t in &trades {
        let effective = t.exit_at.or(t.entry_at).unwrap_or(t.created_at);
        let Some(native_net) = t.net_pnl else {
            open_count += 1;
            continue;
        };
        let Some(net) = fx
            .convert(pool, native_net, &t.currency, display_currency, effective.date())
            .await?
        else {
            unconverted += 1;
            continue;
        };
        let fees = fx
            .convert(
                pool,
                journal::trade_total_fees(t),
                &t.currency,
                display_currency,
                effective.date(),
            )
            .await?
            .unwrap_or(0.0);
        let notional = fx
            .convert(pool, trade_notional(t), &t.currency, display_currency, effective.date())
            .await?
            .unwrap_or(0.0);
        // R stays inside the trade's own currency: a ratio needs no conversion, and this
        // keeps R available even on a date the FX table cannot price.
        let risk_native = trade_risk(t);
        let r = risk_native.map(|risk| native_net / risk);
        let risk_pct = match risk_native {
            Some(risk) if capital > 0.0 => fx
                .convert(pool, risk, &t.currency, display_currency, effective.date())
                .await?
                .map(|converted| converted / capital * 100.0),
            _ => None,
        };
        let hold_min = match (t.entry_at, t.exit_at) {
            (Some(a), Some(b)) if b >= a => Some((b - a).whole_minutes()),
            _ => None,
        };
        closed.push(Closed {
            id: t.id,
            net,
            fees,
            r,
            risk_pct,
            at: effective - offset,
            at_utc: effective,
            entry_at: t.entry_at.map(|e| e - offset),
            entry_at_utc: t.entry_at,
            hold_min,
            notional,
            qty: journal::trade_entry_qty(t),
            leverage: t.leverage,
            entry_price: t.avg_entry,
            exit_price: journal::trade_avg_exit(t),
            strategy_id: t.strategy_id,
            ticker: t.ticker.clone(),
            asset_class: t.asset_class.clone(),
            side: t.side.clone(),
            tags: t.tags.clone(),
        });
    }
    closed.sort_by_key(|c| c.at);

    Ok(Loaded {
        closed,
        invested_capital: capital,
        trade_count: trades.len() as i64,
        open_count,
        unconverted,
    })
}

// ── Output types ─────────────────────────────────────────────────────────────

/// One bar of a distribution or one slice of a histogram. `lo`/`hi` carry the numeric
/// bounds when the label has to be formatted client-side (money, R).
#[derive(Debug, Serialize)]
pub struct Bucket {
    /// Stable identifier for the bucket (i18n key suffix, hour number, weekday number).
    pub key: String,
    pub trades: i64,
    pub wins: i64,
    pub net: f64,
    /// Mean net PnL of the bucket.
    pub avg: f64,
    pub win_rate: Option<f64>,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
}

impl Bucket {
    fn new(key: impl Into<String>) -> Self {
        Bucket {
            key: key.into(),
            trades: 0,
            wins: 0,
            net: 0.0,
            avg: 0.0,
            win_rate: None,
            lo: None,
            hi: None,
        }
    }
    fn push(&mut self, net: f64) {
        self.trades += 1;
        if net > 0.0 {
            self.wins += 1;
        }
        self.net += net;
    }
    fn seal(mut self) -> Self {
        if self.trades > 0 {
            self.avg = self.net / self.trades as f64;
            self.win_rate = Some(self.wins as f64 / self.trades as f64 * 100.0);
        }
        self
    }
}

/// Aggregate for one group (strategy, ticker, asset class, side, tag).
#[derive(Debug, Serialize)]
pub struct GroupRow {
    pub key: String,
    pub name: String,
    /// Extra qualifier: the tag kind for tag rows, empty otherwise.
    pub kind: String,
    pub trades: i64,
    pub wins: i64,
    pub losses: i64,
    pub net: f64,
    pub fees: f64,
    pub win_rate: Option<f64>,
    pub expectancy: Option<f64>,
    pub profit_factor: Option<f64>,
    pub avg_r: Option<f64>,
    pub best: Option<f64>,
    pub worst: Option<f64>,
    /// Share of the filtered closed trades that fall in this group, in percent.
    pub share: Option<f64>,
}

#[derive(Default)]
struct GroupAcc {
    name: String,
    kind: String,
    trades: i64,
    wins: i64,
    losses: i64,
    net: f64,
    fees: f64,
    gross_win: f64,
    gross_loss: f64,
    r_sum: f64,
    r_count: i64,
    best: Option<f64>,
    worst: Option<f64>,
}

impl GroupAcc {
    fn push(&mut self, c: &Closed) {
        self.trades += 1;
        self.net += c.net;
        self.fees += c.fees;
        if c.net > 0.0 {
            self.wins += 1;
            self.gross_win += c.net;
        } else if c.net < 0.0 {
            self.losses += 1;
            self.gross_loss += c.net.abs();
        }
        if let Some(r) = c.r {
            self.r_sum += r;
            self.r_count += 1;
        }
        self.best = Some(self.best.map_or(c.net, |b: f64| b.max(c.net)));
        self.worst = Some(self.worst.map_or(c.net, |w: f64| w.min(c.net)));
    }
    fn seal(self, key: String, total: i64) -> GroupRow {
        GroupRow {
            key,
            name: self.name,
            kind: self.kind,
            trades: self.trades,
            wins: self.wins,
            losses: self.losses,
            net: self.net,
            fees: self.fees,
            win_rate: (self.trades > 0).then(|| self.wins as f64 / self.trades as f64 * 100.0),
            expectancy: (self.trades > 0).then(|| self.net / self.trades as f64),
            profit_factor: (self.gross_loss > 0.0).then(|| self.gross_win / self.gross_loss),
            avg_r: (self.r_count > 0).then(|| self.r_sum / self.r_count as f64),
            best: self.best,
            worst: self.worst,
            share: (total > 0).then(|| self.trades as f64 / total as f64 * 100.0),
        }
    }
}

/// R-multiple stats over the closed trades that carried a planned stop.
#[derive(Debug, Serialize)]
pub struct RStats {
    pub with_stop: i64,
    pub without_stop: i64,
    /// Mean R = expectancy expressed in units of risk.
    pub avg_r: Option<f64>,
    pub total_r: f64,
    pub best_r: Option<f64>,
    pub worst_r: Option<f64>,
    pub avg_win_r: Option<f64>,
    pub avg_loss_r: Option<f64>,
    pub win_rate: Option<f64>,
    /// Mean risk per trade as a percentage of invested capital.
    pub avg_risk_pct: Option<f64>,
    /// Histogram over R, from deep losers to big winners.
    pub buckets: Vec<Bucket>,
}

/// Consecutive wins / losses over the closed trades in exit order.
#[derive(Debug, Serialize)]
pub struct Streaks {
    /// Signed run in progress: +3 = three wins, −2 = two losses, 0 = none.
    pub current: i64,
    pub best_win: i64,
    pub worst_loss: i64,
    pub avg_win: Option<f64>,
    pub avg_loss: Option<f64>,
    /// Best winning run's net PnL, and worst losing run's.
    pub best_win_net: Option<f64>,
    pub worst_loss_net: Option<f64>,
}

/// Risk-adjusted ratios over *trading days* (days with at least one closed trade).
/// Annualized with 252 trading days, the convention every journal and backtester uses.
#[derive(Debug, Serialize)]
pub struct DailyStats {
    pub days: i64,
    pub win_days: i64,
    pub loss_days: i64,
    pub avg_day: Option<f64>,
    pub best_day: Option<f64>,
    pub worst_day: Option<f64>,
    pub sharpe: Option<f64>,
    pub sortino: Option<f64>,
    /// Max peak-to-trough decline of the daily equity, in percent.
    pub max_drawdown: Option<f64>,
    /// Longest stretch, in trading days, spent below a previous equity peak.
    pub max_drawdown_days: i64,
}

#[derive(Debug, Serialize)]
pub struct Distributions {
    pub hold: Vec<Bucket>,
    pub hour: Vec<Bucket>,
    pub weekday: Vec<Bucket>,
    pub size: Vec<Bucket>,
}

#[derive(Debug, Serialize)]
pub struct Groups {
    pub strategy: Vec<GroupRow>,
    pub ticker: Vec<GroupRow>,
    pub asset_class: Vec<GroupRow>,
    pub side: Vec<GroupRow>,
    pub tag: Vec<GroupRow>,
}

/// What breaking the rules costs, in money.
#[derive(Debug, Serialize)]
pub struct MistakeSummary {
    /// Closed trades carrying at least one `mistake` tag, and those carrying none.
    pub tagged: i64,
    pub clean: i64,
    pub tagged_net: f64,
    pub clean_net: f64,
    pub tagged_expectancy: Option<f64>,
    pub clean_expectancy: Option<f64>,
    /// What the tagged trades would have made at the clean expectancy, minus what they
    /// actually made. Positive = the mistakes cost that much.
    pub cost: Option<f64>,
    /// Share of closed trades logged without a mistake tag, in percent.
    pub adherence: Option<f64>,
}

/// One closed trade as plottable scalars: every axis the scatter can pick, plus the
/// labels its tooltip and its colouring need. Money is in the display currency, prices
/// and quantity in the trade's own units. Timestamps are epoch milliseconds, so the
/// client reads them back in its own timezone.
#[derive(Debug, Serialize)]
pub struct TradePoint {
    pub id: Uuid,
    /// Effective instant: exit, else entry, else creation.
    pub at: i64,
    pub entry_at: Option<i64>,
    pub net: f64,
    pub fees: f64,
    pub r: Option<f64>,
    pub risk_pct: Option<f64>,
    pub hold_min: Option<i64>,
    pub notional: f64,
    pub qty: f64,
    pub leverage: f64,
    pub entry_price: Option<f64>,
    pub exit_price: Option<f64>,
    pub ticker: String,
    pub asset_class: String,
    pub side: String,
    /// Strategy name, empty when the trade carries none.
    pub strategy: String,
}

#[derive(Debug, Serialize)]
pub struct Analytics {
    pub display_currency: String,
    pub trade_count: i64,
    pub closed_count: i64,
    pub open_count: i64,
    pub unconverted_trades: i64,
    pub invested_capital: f64,
    pub r: RStats,
    pub streaks: Streaks,
    pub daily: DailyStats,
    pub distributions: Distributions,
    pub groups: Groups,
    pub mistakes: MistakeSummary,
    /// Every closed trade, oldest first. The scatter plots these directly, and the PnL
    /// histogram buckets them client-side: raw rows are what lets the reader change an
    /// axis or a bucket width without a round trip.
    pub points: Vec<TradePoint>,
}

// ── Analytics ────────────────────────────────────────────────────────────────

/// Upper bounds (in minutes) of the holding-time buckets; the last bucket is open-ended.
const HOLD_EDGES: [(i64, &str); 7] = [
    (5, "m5"),
    (30, "m30"),
    (120, "h2"),
    (1440, "d1"),
    (4320, "d3"),
    (10080, "w1"),
    (i64::MAX, "w1plus"),
];

/// R histogram edges. Bounded on both sides so the frontend can draw a real axis; the
/// two extremes are the open-ended tails.
const R_EDGES: [(f64, f64, &str); 8] = [
    (f64::NEG_INFINITY, -3.0, "lt3"),
    (-3.0, -2.0, "m3m2"),
    (-2.0, -1.0, "m2m1"),
    (-1.0, 0.0, "m1z"),
    (0.0, 1.0, "z1"),
    (1.0, 2.0, "r1r2"),
    (2.0, 3.0, "r2r3"),
    (3.0, f64::INFINITY, "gt3"),
];

/// Sum whose empty case is +0.0. `Iterator::sum` folds from −0.0 (the identity that keeps
/// every term's sign), so a period with no trade would otherwise serialize as `-0.0` and
/// render as "−0.00" on screen.
fn total<I: IntoIterator<Item = f64>>(it: I) -> f64 {
    it.into_iter().fold(0.0, |a, b| a + b)
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn r_stats(closed: &[Closed]) -> RStats {
    let rs: Vec<f64> = closed.iter().filter_map(|c| c.r).collect();
    let with_stop = rs.len() as i64;
    let without_stop = closed.len() as i64 - with_stop;
    let wins: Vec<f64> = rs.iter().copied().filter(|r| *r > 0.0).collect();
    let losses: Vec<f64> = rs.iter().copied().filter(|r| *r < 0.0).collect();
    let risk_pcts: Vec<f64> = closed.iter().filter_map(|c| c.risk_pct).collect();

    let mut buckets: Vec<Bucket> = R_EDGES
        .iter()
        .map(|(lo, hi, key)| {
            let mut b = Bucket::new(*key);
            b.lo = lo.is_finite().then_some(*lo);
            b.hi = hi.is_finite().then_some(*hi);
            b
        })
        .collect();
    for c in closed {
        let Some(r) = c.r else { continue };
        // Half-open [lo, hi) so exactly one bucket claims each R.
        let idx = R_EDGES
            .iter()
            .position(|(lo, hi, _)| r >= *lo && r < *hi)
            .unwrap_or(R_EDGES.len() - 1);
        buckets[idx].push(c.net);
    }

    RStats {
        with_stop,
        without_stop,
        avg_r: mean(&rs),
        total_r: total(rs.iter().copied()),
        best_r: rs.iter().copied().reduce(f64::max),
        worst_r: rs.iter().copied().reduce(f64::min),
        avg_win_r: mean(&wins),
        avg_loss_r: mean(&losses),
        win_rate: (with_stop > 0).then(|| wins.len() as f64 / with_stop as f64 * 100.0),
        avg_risk_pct: mean(&risk_pcts),
        buckets: buckets.into_iter().map(Bucket::seal).collect(),
    }
}

fn streaks(closed: &[Closed]) -> Streaks {
    // A breakeven trade (net exactly 0) ends both runs without starting one — it is
    // neither a win nor a loss, and pretending otherwise inflates a streak.
    let mut current = 0i64;
    let mut current_net = 0.0;
    let mut best_win = 0i64;
    let mut worst_loss = 0i64;
    let mut best_win_net: Option<f64> = None;
    let mut worst_loss_net: Option<f64> = None;
    let mut win_runs: Vec<f64> = Vec::new();
    let mut loss_runs: Vec<f64> = Vec::new();

    let flush = |run: i64, win_runs: &mut Vec<f64>, loss_runs: &mut Vec<f64>| {
        if run > 0 {
            win_runs.push(run as f64);
        } else if run < 0 {
            loss_runs.push(-run as f64);
        }
    };

    for c in closed {
        let dir = if c.net > 0.0 {
            1
        } else if c.net < 0.0 {
            -1
        } else {
            0
        };
        if dir == 0 {
            flush(current, &mut win_runs, &mut loss_runs);
            current = 0;
            current_net = 0.0;
            continue;
        }
        if current.signum() == dir {
            current += dir;
            current_net += c.net;
        } else {
            flush(current, &mut win_runs, &mut loss_runs);
            current = dir;
            current_net = c.net;
        }
        if current > best_win {
            best_win = current;
            best_win_net = Some(current_net);
        }
        if current < worst_loss {
            worst_loss = current;
            worst_loss_net = Some(current_net);
        }
    }
    flush(current, &mut win_runs, &mut loss_runs);

    Streaks {
        current,
        best_win,
        worst_loss: worst_loss.abs(),
        avg_win: mean(&win_runs),
        avg_loss: mean(&loss_runs),
        best_win_net,
        worst_loss_net,
    }
}

/// Per-day net PnL over the closed trades, in local-date order.
fn daily_series(closed: &[Closed]) -> Vec<(Date, f64)> {
    let mut days: BTreeMap<Date, f64> = BTreeMap::new();
    for c in closed {
        *days.entry(c.at.date()).or_insert(0.0) += c.net;
    }
    days.into_iter().collect()
}

fn daily_stats(closed: &[Closed], invested_capital: f64) -> DailyStats {
    let days = daily_series(closed);
    let nets: Vec<f64> = days.iter().map(|(_, n)| *n).collect();

    let mut equity = invested_capital;
    let mut peak = invested_capital;
    let mut max_dd = 0.0_f64;
    let mut below_peak = 0i64;
    let mut max_below = 0i64;
    for net in &nets {
        equity += net;
        if equity >= peak {
            peak = equity;
            below_peak = 0;
        } else {
            below_peak += 1;
            max_below = max_below.max(below_peak);
            if peak > 0.0 {
                max_dd = max_dd.max((peak - equity) / peak);
            }
        }
    }

    // One definition, shared with the breakdown.
    let (sharpe, sortino) = journal::daily_ratios(&days, invested_capital);

    DailyStats {
        days: days.len() as i64,
        win_days: nets.iter().filter(|n| **n > 0.0).count() as i64,
        loss_days: nets.iter().filter(|n| **n < 0.0).count() as i64,
        avg_day: mean(&nets),
        best_day: nets.iter().copied().reduce(f64::max),
        worst_day: nets.iter().copied().reduce(f64::min),
        sharpe,
        sortino,
        max_drawdown: (!nets.is_empty() && invested_capital > 0.0).then_some(max_dd * 100.0),
        max_drawdown_days: max_below,
    }
}

fn distributions(closed: &[Closed]) -> Distributions {
    // Holding time.
    let mut hold: Vec<Bucket> = HOLD_EDGES
        .iter()
        .map(|(hi, key)| {
            let mut b = Bucket::new(*key);
            b.hi = (*hi != i64::MAX).then(|| *hi as f64);
            b
        })
        .collect();
    for c in closed {
        let Some(m) = c.hold_min else { continue };
        let idx = HOLD_EDGES.iter().position(|(hi, _)| m < *hi).unwrap_or(HOLD_EDGES.len() - 1);
        hold[idx].push(c.net);
    }

    // Entry hour (local) and weekday (Monday = 0).
    let mut hour: Vec<Bucket> = (0..24).map(|h| Bucket::new(h.to_string())).collect();
    let mut weekday: Vec<Bucket> = (0..7).map(|d| Bucket::new(d.to_string())).collect();
    for c in closed {
        let Some(e) = c.entry_at else { continue };
        hour[e.hour() as usize].push(c.net);
        weekday[e.weekday().number_days_from_monday() as usize].push(c.net);
    }

    // Position size: five quantile slices of the entry notional, so the buckets adapt to
    // whatever the account trades instead of assuming a scale.
    let mut sizes: Vec<f64> = closed.iter().map(|c| c.notional).filter(|n| *n > 0.0).collect();
    sizes.sort_by(f64::total_cmp);
    let mut size: Vec<Bucket> = Vec::new();
    if !sizes.is_empty() {
        let slices = 5.min(sizes.len());
        let mut edges: Vec<f64> = Vec::new();
        for i in 1..slices {
            edges.push(sizes[i * sizes.len() / slices]);
        }
        // Collapse duplicate edges (many identical sizes) so no empty slice is drawn.
        edges.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
        for i in 0..=edges.len() {
            let mut b = Bucket::new(i.to_string());
            b.lo = if i == 0 { Some(sizes[0]) } else { Some(edges[i - 1]) };
            b.hi = if i == edges.len() {
                sizes.last().copied()
            } else {
                Some(edges[i])
            };
            size.push(b);
        }
        for c in closed {
            if c.notional <= 0.0 {
                continue;
            }
            let idx = edges.iter().position(|e| c.notional < *e).unwrap_or(edges.len());
            size[idx].push(c.net);
        }
    }

    Distributions {
        hold: hold.into_iter().map(Bucket::seal).collect(),
        hour: hour.into_iter().map(Bucket::seal).collect(),
        weekday: weekday.into_iter().map(Bucket::seal).collect(),
        size: size.into_iter().map(Bucket::seal).collect(),
    }
}

/// Build one group table. `key_of` yields the group key and display name; a trade may
/// land in several groups (tags), hence the Vec.
fn group_by<F>(closed: &[Closed], mut key_of: F) -> Vec<GroupRow>
where
    F: FnMut(&Closed) -> Vec<(String, String, String)>,
{
    let mut acc: HashMap<String, GroupAcc> = HashMap::new();
    let total = closed.len() as i64;
    for c in closed {
        for (key, name, kind) in key_of(c) {
            let e = acc.entry(key).or_default();
            if e.name.is_empty() {
                e.name = name;
                e.kind = kind;
            }
            e.push(c);
        }
    }
    let mut rows: Vec<GroupRow> = acc.into_iter().map(|(k, v)| v.seal(k, total)).collect();
    // Most-traded first: the table is read to find where the volume goes.
    rows.sort_by(|a, b| b.trades.cmp(&a.trades).then(b.net.total_cmp(&a.net)));
    rows
}

fn mistake_summary(closed: &[Closed], mistake_tags: &std::collections::HashSet<Uuid>) -> MistakeSummary {
    let (tagged, clean): (Vec<&Closed>, Vec<&Closed>) = closed
        .iter()
        .partition(|c| c.tags.iter().any(|id| mistake_tags.contains(id)));
    let tagged_net = total(tagged.iter().map(|c| c.net));
    let clean_net = total(clean.iter().map(|c| c.net));
    let tagged_expectancy = (!tagged.is_empty()).then(|| tagged_net / tagged.len() as f64);
    let clean_expectancy = (!clean.is_empty()).then(|| clean_net / clean.len() as f64);
    // The counterfactual is deliberately modest: the same trades, at the average result
    // of the trades where no rule was broken. It says what the breaches cost, not what a
    // perfect version of the plan would have made.
    let cost = match (tagged_expectancy, clean_expectancy) {
        (Some(_), Some(clean_e)) => Some(clean_e * tagged.len() as f64 - tagged_net),
        _ => None,
    };
    MistakeSummary {
        tagged: tagged.len() as i64,
        clean: clean.len() as i64,
        tagged_net,
        clean_net,
        tagged_expectancy,
        clean_expectancy,
        cost,
        adherence: (!closed.is_empty()).then(|| clean.len() as f64 / closed.len() as f64 * 100.0),
    }
}

/// The full analytics payload for the filtered trades.
pub async fn analytics(
    pool: &PgPool,
    filter: &TradeFilter,
    display_currency: &str,
    tz_offset_min: i32,
) -> anyhow::Result<Analytics> {
    let loaded = load(pool, filter, display_currency, tz_offset_min).await?;
    let closed = &loaded.closed;

    let strategies = journal::strategy_names(pool).await?;
    let tags = tag_meta(pool).await?;
    let mistake_ids: std::collections::HashSet<Uuid> = tags
        .iter()
        .filter(|(_, (_, kind))| kind == "mistake")
        .map(|(id, _)| *id)
        .collect();

    let by_strategy = group_by(closed, |c| match c.strategy_id {
        Some(id) => vec![(
            id.to_string(),
            strategies.get(&id).cloned().unwrap_or_default(),
            String::new(),
        )],
        None => vec![(String::new(), String::new(), String::new())],
    });
    let by_ticker = group_by(closed, |c| {
        let key = c.ticker.trim().to_uppercase();
        vec![(key.clone(), key, String::new())]
    });
    let by_asset = group_by(closed, |c| {
        vec![(c.asset_class.clone(), c.asset_class.clone(), String::new())]
    });
    let by_side = group_by(closed, |c| vec![(c.side.clone(), c.side.clone(), String::new())]);
    let by_tag = group_by(closed, |c| {
        c.tags
            .iter()
            .map(|id| {
                let (name, kind) = tags.get(id).cloned().unwrap_or_default();
                (id.to_string(), name, kind)
            })
            .collect()
    });

    let points = closed
        .iter()
        .map(|c| TradePoint {
            id: c.id,
            at: c.at_utc.unix_timestamp() * 1000,
            entry_at: c.entry_at_utc.map(|e| e.unix_timestamp() * 1000),
            net: c.net,
            fees: c.fees,
            r: c.r,
            risk_pct: c.risk_pct,
            hold_min: c.hold_min,
            notional: c.notional,
            qty: c.qty,
            leverage: c.leverage,
            entry_price: c.entry_price,
            exit_price: c.exit_price,
            ticker: c.ticker.trim().to_uppercase(),
            asset_class: c.asset_class.clone(),
            side: c.side.clone(),
            strategy: c
                .strategy_id
                .and_then(|id| strategies.get(&id).cloned())
                .unwrap_or_default(),
        })
        .collect();

    Ok(Analytics {
        display_currency: display_currency.to_string(),
        trade_count: loaded.trade_count,
        closed_count: closed.len() as i64,
        open_count: loaded.open_count,
        unconverted_trades: loaded.unconverted,
        invested_capital: loaded.invested_capital,
        r: r_stats(closed),
        streaks: streaks(closed),
        daily: daily_stats(closed, loaded.invested_capital),
        distributions: distributions(closed),
        groups: Groups {
            strategy: by_strategy,
            ticker: by_ticker,
            asset_class: by_asset,
            side: by_side,
            tag: by_tag,
        },
        mistakes: mistake_summary(closed, &mistake_ids),
        points,
    })
}

// ── Period comparison ────────────────────────────────────────────────────────

/// One period's headline numbers. Same shape for the current window, the previous one
/// and every bar of the history strip, so the client compares field by field.
#[derive(Debug, Serialize)]
pub struct PeriodStats {
    /// "YYYY-MM-DD" of the period's first local day, the client's label anchor.
    pub start: String,
    pub end: String,
    pub trades: i64,
    pub wins: i64,
    pub losses: i64,
    pub net: f64,
    pub fees: f64,
    pub win_rate: Option<f64>,
    pub expectancy: Option<f64>,
    pub profit_factor: Option<f64>,
    pub avg_r: Option<f64>,
    pub best: Option<f64>,
    pub worst: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub trading_days: i64,
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub period: String,
    pub display_currency: String,
    pub current: PeriodStats,
    pub previous: PeriodStats,
    /// Oldest first, ending with the current period. Drawn as the bar strip.
    pub history: Vec<PeriodStats>,
}

/// Period granularities the comparison accepts.
pub const PERIODS: [&str; 6] = ["day", "week", "month", "quarter", "year", "custom"];

/// Start of the period containing `d`, shifted back by `back` whole periods.
fn period_start(period: &str, d: Date, back: i64) -> Date {
    match period {
        "day" => d - Duration::days(back),
        "week" => {
            let monday = d - Duration::days(d.weekday().number_days_from_monday() as i64);
            monday - Duration::weeks(back)
        }
        "month" => add_months(first_of_month(d), -back),
        "quarter" => {
            let m = u8::from(d.month());
            let q_first = ((m - 1) / 3) * 3 + 1;
            let start = Date::from_calendar_date(d.year(), month(q_first), 1).unwrap_or(d);
            add_months(start, -back * 3)
        }
        // "year" and anything unexpected fall back to a calendar year.
        _ => Date::from_calendar_date(d.year() - back as i32, time::Month::January, 1)
            .unwrap_or(d),
    }
}

fn month(m: u8) -> time::Month {
    time::Month::try_from(m.clamp(1, 12)).unwrap_or(time::Month::January)
}

fn first_of_month(d: Date) -> Date {
    d.replace_day(1).unwrap_or(d)
}

/// Shift a first-of-month date by `n` months (negative = back).
fn add_months(d: Date, n: i64) -> Date {
    let total = d.year() as i64 * 12 + (u8::from(d.month()) as i64 - 1) + n;
    let year = total.div_euclid(12) as i32;
    let m = total.rem_euclid(12) as u8 + 1;
    Date::from_calendar_date(year, month(m), 1).unwrap_or(d)
}

/// Exclusive end of the period starting at `start`.
fn period_end(period: &str, start: Date) -> Date {
    match period {
        "day" => start + Duration::days(1),
        "week" => start + Duration::weeks(1),
        "month" => add_months(start, 1),
        "quarter" => add_months(start, 3),
        _ => Date::from_calendar_date(start.year() + 1, time::Month::January, 1).unwrap_or(start),
    }
}

fn ymd(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Reduce the closed trades whose local date falls in [start, end) to one row.
fn period_stats(closed: &[Closed], start: Date, end: Date) -> PeriodStats {
    let slice: Vec<&Closed> = closed
        .iter()
        .filter(|c| {
            let d = c.at.date();
            d >= start && d < end
        })
        .collect();
    let net = total(slice.iter().map(|c| c.net));
    let fees = total(slice.iter().map(|c| c.fees));
    let wins = slice.iter().filter(|c| c.net > 0.0).count() as i64;
    let losses = slice.iter().filter(|c| c.net < 0.0).count() as i64;
    let gross_win = total(slice.iter().filter(|c| c.net > 0.0).map(|c| c.net));
    let gross_loss = total(slice.iter().filter(|c| c.net < 0.0).map(|c| -c.net));
    let rs: Vec<f64> = slice.iter().filter_map(|c| c.r).collect();
    let n = slice.len() as i64;

    // Drawdown inside the window, over cumulative PnL (a window has no capital of its
    // own): peak-to-trough of the running total, as a share of the peak.
    let mut cum = 0.0;
    let mut peak = 0.0_f64;
    let mut max_dd = 0.0_f64;
    for c in &slice {
        cum += c.net;
        peak = peak.max(cum);
        if peak > 0.0 {
            max_dd = max_dd.max((peak - cum) / peak);
        }
    }
    let days: std::collections::HashSet<Date> = slice.iter().map(|c| c.at.date()).collect();

    PeriodStats {
        start: ymd(start),
        end: ymd(end),
        trades: n,
        wins,
        losses,
        net,
        fees,
        win_rate: (n > 0).then(|| wins as f64 / n as f64 * 100.0),
        expectancy: (n > 0).then(|| net / n as f64),
        profit_factor: (gross_loss > 0.0).then(|| gross_win / gross_loss),
        avg_r: mean(&rs),
        best: slice.iter().map(|c| c.net).reduce(f64::max),
        worst: slice.iter().map(|c| c.net).reduce(f64::min),
        max_drawdown: (n > 0 && peak > 0.0).then_some(max_dd * 100.0),
        trading_days: days.len() as i64,
    }
}

/// How many periods of history the strip carries, current one included.
const HISTORY_LEN: i64 = 12;

/// Compare a period against the one before it, plus a strip of recent periods.
///
/// `period` is one of `PERIODS`. For `custom`, the window is the filter's own
/// `since`/`until` and the comparison window is the same length immediately before, so
/// "these three weeks vs the three before" needs no extra parameter.
pub async fn compare(
    pool: &PgPool,
    filter: &TradeFilter,
    period: &str,
    anchor: Date,
    display_currency: &str,
    tz_offset_min: i32,
) -> anyhow::Result<Comparison> {
    let (cur_start, cur_end, prev_start, prev_end, history): (Date, Date, Date, Date, Vec<(Date, Date)>) =
        if period == "custom" {
            let offset = Duration::minutes(tz_offset_min as i64);
            let start = filter
                .since
                .map(|s| (s - offset).date())
                .unwrap_or_else(|| anchor - Duration::days(30));
            // `until` is inclusive in the shared filter; the window is half-open.
            let end = filter
                .until
                .map(|u| (u - offset).date() + Duration::days(1))
                .unwrap_or(anchor + Duration::days(1));
            let len = (end - start).whole_days().max(1);
            let prev_start = start - Duration::days(len);
            (start, end, prev_start, start, vec![(prev_start, start), (start, end)])
        } else {
            let cur_start = period_start(period, anchor, 0);
            let cur_end = period_end(period, cur_start);
            let prev_start = period_start(period, anchor, 1);
            let prev_end = period_end(period, prev_start);
            let mut history: Vec<(Date, Date)> = (0..HISTORY_LEN)
                .map(|back| {
                    let s = period_start(period, anchor, back);
                    (s, period_end(period, s))
                })
                .collect();
            history.reverse(); // oldest first
            (cur_start, cur_end, prev_start, prev_end, history)
        };

    // One load covering the whole strip, sliced in memory: the comparison must never
    // fan out into one query per period.
    let span_start = history.first().map(|(s, _)| *s).unwrap_or(prev_start);
    let offset = Duration::minutes(tz_offset_min as i64);
    let span_filter = TradeFilter {
        category_id: filter.category_id,
        strategy_id: filter.strategy_id,
        asset_class: filter.asset_class.clone(),
        side: filter.side.clone(),
        ticker: filter.ticker.clone(),
        signal_name: filter.signal_name.clone(),
        tag_id: filter.tag_id,
        since: Some(span_start.midnight().assume_utc() + offset),
        until: Some(cur_end.max(prev_end).midnight().assume_utc() + offset),
    };
    let loaded = load(pool, &span_filter, display_currency, tz_offset_min).await?;

    Ok(Comparison {
        period: period.to_string(),
        display_currency: display_currency.to_string(),
        current: period_stats(&loaded.closed, cur_start, cur_end),
        previous: period_stats(&loaded.closed, prev_start, prev_end),
        history: history
            .into_iter()
            .map(|(s, e)| period_stats(&loaded.closed, s, e))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(net: f64, r: Option<f64>) -> Closed {
        Closed {
            id: Uuid::nil(),
            net,
            fees: 0.0,
            r,
            risk_pct: None,
            at: OffsetDateTime::UNIX_EPOCH,
            at_utc: OffsetDateTime::UNIX_EPOCH,
            entry_at: None,
            entry_at_utc: None,
            hold_min: None,
            notional: 0.0,
            qty: 0.0,
            leverage: 1.0,
            entry_price: None,
            exit_price: None,
            strategy_id: None,
            ticker: String::new(),
            asset_class: String::new(),
            side: String::new(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn streaks_break_on_breakeven() {
        let trades = vec![c(1.0, None), c(2.0, None), c(0.0, None), c(3.0, None)];
        let s = streaks(&trades);
        assert_eq!(s.best_win, 2);
        assert_eq!(s.current, 1);
    }

    #[test]
    fn streaks_track_the_worst_run() {
        let trades = vec![c(-1.0, None), c(-2.0, None), c(-3.0, None), c(5.0, None)];
        let s = streaks(&trades);
        assert_eq!(s.worst_loss, 3);
        assert_eq!(s.current, 1);
    }

    #[test]
    fn r_buckets_are_half_open() {
        let trades = vec![c(1.0, Some(1.0)), c(-1.0, Some(-1.0)), c(9.0, Some(4.5))];
        let s = r_stats(&trades);
        assert_eq!(s.with_stop, 3);
        // R = 1.0 lands in [1, 2), R = −1.0 in [−1, 0), R = 4.5 in the > 3 tail.
        let by: std::collections::HashMap<_, _> =
            s.buckets.iter().map(|b| (b.key.as_str(), b.trades)).collect();
        assert_eq!(by["r1r2"], 1);
        assert_eq!(by["m1z"], 1);
        assert_eq!(by["gt3"], 1);
    }

    #[test]
    fn an_empty_period_is_a_positive_zero() {
        // `Iterator::sum` folds f64 from −0.0; a period with no trade must not serialize
        // as -0.0 and render as "−0.00".
        let start = Date::from_calendar_date(2026, time::Month::January, 1).unwrap();
        let end = Date::from_calendar_date(2026, time::Month::February, 1).unwrap();
        let p = period_stats(&[], start, end);
        assert!(p.net.is_sign_positive());
        assert!(p.fees.is_sign_positive());
        assert_eq!(p.trades, 0);
    }

    #[test]
    fn add_months_wraps_the_year() {
        let d = Date::from_calendar_date(2026, time::Month::November, 1).unwrap();
        assert_eq!(u8::from(add_months(d, 3).month()), 2);
        assert_eq!(add_months(d, 3).year(), 2027);
        assert_eq!(u8::from(add_months(d, -11).month()), 12);
        assert_eq!(add_months(d, -11).year(), 2025);
    }

    #[test]
    fn mistake_cost_is_the_gap_to_clean_trades() {
        let tag = Uuid::new_v4();
        let mut bad = c(-100.0, None);
        bad.tags = vec![tag];
        let trades = vec![bad, c(50.0, None), c(50.0, None)];
        let set: std::collections::HashSet<Uuid> = [tag].into_iter().collect();
        let m = mistake_summary(&trades, &set);
        // One tagged trade at −100 against a clean expectancy of +50 costs 150.
        assert_eq!(m.cost, Some(150.0));
        assert_eq!(m.tagged, 1);
        assert_eq!(m.clean, 2);
    }
}
